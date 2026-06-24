#![doc = include_str!("../README.md")]

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::error::Error;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;

use fst::{Automaton, IntoStreamer, Set, SetBuilder, Streamer};
use levenshtein_automata::{DFA, Distance, LevenshteinAutomatonBuilder};
use memmap2::Mmap;
use rayon::prelude::*;

/// A wrapper implementing [`fst::Automaton`] for a Levenshtein [`DFA`].
pub struct FstDfaWrapper(pub DFA);

impl fst::Automaton for FstDfaWrapper {
    type State = u32;

    #[inline]
    fn start(&self) -> Self::State {
        self.0.initial_state()
    }

    #[inline]
    fn is_match(&self, state: &Self::State) -> bool {
        matches!(self.0.distance(*state), Distance::Exact(_))
    }

    #[inline]
    fn accept(&self, state: &Self::State, byte: u8) -> Self::State {
        self.0.transition(*state, byte)
    }

    #[inline]
    fn can_match(&self, state: &Self::State) -> bool {
        *state != levenshtein_automata::SINK_STATE
    }
}

/// A memory-mapped dictionary for fuzzy string lookups.
pub struct Dictionary {
    map: Set<Mmap>,
    lev_builders: Vec<LevenshteinAutomatonBuilder>,
}

/// An individual match from a fuzzy search query.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SearchResult {
    /// Indicates if the candidate matches the query exactly (distance of 0).
    pub is_exact: bool,
    /// The matched text from the dictionary.
    pub key: String,
}

/// A builder for configuring and running a dictionary search query.
pub struct SearchBuilder<'a> {
    dictionary: &'a Dictionary,
    query: String,
    limit: usize,
    distance: u8,
}

impl<'a> SearchBuilder<'a> {
    /// Create a new [`SearchBuilder`] with default limits (5) and distance (1).
    pub fn new(dictionary: &'a Dictionary, query: &str) -> Self {
        Self {
            dictionary,
            query: query.to_string(),
            limit: 5,
            distance: 1,
        }
    }

    /// Set the maximum number of results to return.
    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }

    /// Set the maximum Levenshtein distance allowed for typos.
    pub fn distance(mut self, distance: u8) -> Self {
        self.distance = distance;
        self
    }

    /// Execute the fuzzy search query.
    pub fn execute(self) -> Result<Vec<SearchResult>, Box<dyn Error>> {
        let dfa = if let Some(builder) = self.dictionary.lev_builders.get(self.distance as usize) {
            FstDfaWrapper(builder.build_dfa(&self.query))
        } else {
            let builder = LevenshteinAutomatonBuilder::new(self.distance, false);
            FstDfaWrapper(builder.build_dfa(&self.query))
        };

        let mut heap = BinaryHeap::with_capacity(self.limit);
        let mut stream = self.dictionary.map.search(&dfa).into_stream();

        while let Some(key_bytes) = stream.next() {
            let mut state = dfa.start();
            for &byte in key_bytes {
                state = dfa.accept(&state, byte);
            }

            let dist = match dfa.0.distance(state) {
                levenshtein_automata::Distance::Exact(d) => d,
                _ => self.distance,
            };

            let candidate = (dist, key_bytes);

            if heap.len() < self.limit {
                heap.push((dist, key_bytes.to_vec()));
            } else if let Some(mut worst) = heap.peek_mut()
                && candidate < (worst.0, worst.1.as_slice())
            {
                *worst = (dist, key_bytes.to_vec());
            }
        }

        let mut results: Vec<_> = heap
            .into_iter()
            .map(|(dist, bytes)| {
                Ok(SearchResult {
                    is_exact: dist == 0,
                    key: String::from_utf8(bytes)?,
                })
            })
            .collect::<Result<_, Box<dyn Error>>>()?;

        results.sort_unstable_by(|a, b| {
            (Reverse(a.is_exact), &a.key).cmp(&(Reverse(b.is_exact), &b.key))
        });

        Ok(results)
    }
}

impl Dictionary {
    /// Open an existing compiled FST dictionary file via memory mapping.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Box<dyn Error>> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };
        let map = Set::new(mmap)?;

        let lev_builders = (0..=2)
            .map(|d| LevenshteinAutomatonBuilder::new(d, false))
            .collect();

        Ok(Self { map, lev_builders })
    }

    /// Sorts a line-delimited text file in-place in lexicographical byte order.
    /// This prepares an unsorted text file to be compatible with [`Dictionary::build`].
    ///
    /// # Warning
    ///
    /// This function reads the entire contents of the file into system memory (RAM). It is
    /// **not suitable for large dictionaries** and may cause out-of-memory panics if the file size
    /// exceeds available memory. For large datasets, users should pre-sort the file via external
    /// means—such as the standard command-line `sort` utility—before processing.
    pub fn sort(path: impl AsRef<Path>) -> Result<(), Box<dyn Error>> {
        let path = path.as_ref();
        let file = File::open(path)?;
        let reader = BufReader::new(file);

        let mut contents: Vec<String> = reader.lines().collect::<Result<Vec<_>, _>>()?;
        contents.sort_unstable();

        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);

        for content in contents {
            writeln!(writer, "{}", content)?;
        }

        Ok(())
    }

    /// Compile a line-delimited text file into an immutable binary FST file.
    ///
    /// # Note
    ///
    /// The input text file lines must be sorted in lexicographical byte order.
    pub fn build(
        input_path: impl AsRef<Path>,
        output_path: impl AsRef<Path>,
    ) -> Result<(), Box<dyn Error>> {
        let mut reader = BufReader::new(File::open(input_path)?);
        let mut build = SetBuilder::new(BufWriter::new(File::create(output_path)?))?;
        let mut line = String::new();

        while reader.read_line(&mut line)? > 0 {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                build.insert(trimmed)?;
            }
            line.clear();
        }

        build.finish()?;
        Ok(())
    }

    /// Create a search query builder for this dictionary.
    pub fn search<'a>(&'a self, query: &str) -> SearchBuilder<'a> {
        SearchBuilder::new(self, query)
    }

    /// Run multiple search queries concurrently using a parallel thread pool.
    pub fn batch_search(
        &self,
        queries: &[&str],
    ) -> Vec<Result<Vec<SearchResult>, Box<dyn Error + Send + Sync>>> {
        queries
            .par_iter()
            .map(|&query| {
                self.search(query)
                    .execute()
                    .map_err(|e| e.to_string().into())
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn create_test_dict(words: &[&str]) -> Dictionary {
        let mut sorted = words.to_vec();
        sorted.sort_unstable();

        let mut buffer = Vec::new();
        let mut build = SetBuilder::new(&mut buffer).unwrap();
        for word in sorted {
            build.insert(word).unwrap();
        }
        build.finish().unwrap();

        let mut mmap = memmap2::MmapMut::map_anon(buffer.len()).unwrap();
        mmap.copy_from_slice(&buffer);
        let mmap = mmap.make_read_only().unwrap();

        let lev_builders = (0..=2)
            .map(|d| LevenshteinAutomatonBuilder::new(d, false))
            .collect();

        Dictionary {
            map: Set::new(mmap).unwrap(),
            lev_builders,
        }
    }

    #[test]
    fn test_dictionary_search() {
        let dict = create_test_dict(&["apple", "banana", "lime", "time", "mime", "cherry"]);

        // Exact match
        let results = dict.search("apple").execute().unwrap();
        let keys: Vec<&str> = results.iter().map(|r| r.key.as_ref()).collect();
        assert_eq!(keys, vec!["apple"]);

        // Levenshtein distance of 1 (substitution)
        let results = dict.search("baxana").execute().unwrap();
        let keys: Vec<&str> = results.iter().map(|r| r.key.as_ref()).collect();
        assert_eq!(keys, vec!["banana"]);

        // Levenshtein distance of 1 (deletion/substitution)
        let results = dict.search("cheriy").execute().unwrap();
        let keys: Vec<&str> = results.iter().map(|r| r.key.as_ref()).collect();
        assert_eq!(keys, vec!["cherry"]);

        // Dynamic distance of 2
        let results = dict.search("baxaxa").distance(2).execute().unwrap();
        let keys: Vec<&str> = results.iter().map(|r| r.key.as_ref()).collect();
        assert_eq!(keys, vec!["banana"]);

        // Out of bounds (> 1 distance)
        let results = dict.search("ap").execute().unwrap();
        assert!(results.is_empty());

        // With limit of 3
        let results = dict.search("mime").limit(3).execute().unwrap();
        let keys: Vec<&str> = results.iter().map(|r| r.key.as_ref()).collect();
        assert_eq!(keys, vec!["mime", "lime", "time"]);
    }

    #[test]
    fn test_batch_search() {
        let dict = create_test_dict(&["apple", "banana", "cherry"]);

        let batch_queries: Vec<&str> = (0..10)
            .map(|i| if i % 2 == 0 { "word0050" } else { "baxana" })
            .collect();

        let results = dict.batch_search(&batch_queries);
        // Should received 10 responses
        assert_eq!(results.len(), 10);

        for (i, res) in results.into_iter().enumerate() {
            let matches = res.expect("Search thread panicked or errored unexpectedly");

            if i % 2 == 0 {
                // "word0050" is outside Levenshtein distance 1 for all items
                assert!(matches.is_empty(), "Expected empty results for index {}", i);
            } else {
                // "baxana" should successfully resolve to "banana"
                assert_eq!(matches.len(), 1, "Expected exactly 1 match for index {}", i);
                assert_eq!(matches[0].key, "banana");
                assert_eq!(matches[0].is_exact, false);
            }
        }
    }

    #[test]
    fn test_dictionary_sort() {
        // Create an unsorted temporary file
        let input_words = vec!["ßé", "àé", "äb"];
        let mut source_file = tempfile::NamedTempFile::new().unwrap();
        for word in &input_words {
            writeln!(source_file, "{}", word).unwrap();
        }

        // Execute the in-place sort
        Dictionary::sort(source_file.path()).unwrap();

        // Read the file back to verify the final order
        let file = File::open(source_file.path()).unwrap();
        let reader = BufReader::new(file);
        let sorted_lines: Vec<String> = reader.lines().collect::<Result<Vec<_>, _>>().unwrap();

        // Verify it matches lexicographical byte order
        assert_eq!(
            sorted_lines,
            vec!["ßé".to_string(), "àé".to_string(), "äb".to_string(),]
        );
    }

    #[test]
    fn test_path_generics() {
        let mut source_file = tempfile::NamedTempFile::new().unwrap();
        writeln!(source_file, "apple\nbanana\ncherry").unwrap();

        let target_dir = tempfile::tempdir().unwrap();
        let target_fst_path = target_dir.path().join("dict.fst");

        // === Test Dictionary::build with various types matching `impl AsRef<Path>` ===

        // Using &NamedTempFile and &PathBuf
        Dictionary::build(&source_file, &target_fst_path).unwrap();
        assert!(target_fst_path.exists());

        // Using &Path and PathBuf (moving the target path)
        let source_path: &std::path::Path = source_file.path();
        Dictionary::build(source_path, target_fst_path.clone()).unwrap();

        // Using &str and String representations
        let source_str: &str = source_path.to_str().unwrap();
        let target_string: String = target_fst_path.to_str().unwrap().to_string();
        Dictionary::build(source_str, &target_string).unwrap();

        // === Test Dictionary::open with various types matching `impl AsRef<Path>` ===

        // Test with &PathBuf
        let dict1 = Dictionary::open(&target_fst_path).unwrap();
        assert_eq!(dict1.search("apple").execute().unwrap().len(), 1);

        // Test with owned PathBuf
        let dict2 = Dictionary::open(target_fst_path.clone()).unwrap();
        assert_eq!(dict2.search("baxana").execute().unwrap()[0].key, "banana");

        // Test with string primitives (&str and String)
        let dict3 = Dictionary::open(target_string.as_str()).unwrap();
        assert_eq!(dict3.search("cheriy").execute().unwrap().len(), 1);

        let dict4 = Dictionary::open(target_string).unwrap();
        assert!(!dict4.search("cherry").execute().unwrap().is_empty());
    }

    #[test]
    fn test_distance_priority_over_alphabetical() {
        let dict = create_test_dict(&["east", "fest"]);

        let results = dict.search("test").distance(2).limit(1).execute().unwrap();
        let keys: Vec<&str> = results.iter().map(|r| r.key.as_ref()).collect();

        // "fest" should win because a distance of 1 is a better match than 2.
        assert_eq!(keys, vec!["fest"]);
    }
}
