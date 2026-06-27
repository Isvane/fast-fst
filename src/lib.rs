#![doc = include_str!("../README.md")]

use std::collections::BinaryHeap;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;

use fst::{Automaton, IntoStreamer, Set, SetBuilder, Streamer};
use levenshtein_automata::{DFA, Distance, LevenshteinAutomatonBuilder};
use memmap2::Mmap;
use rayon::prelude::*;

#[derive(thiserror::Error, Debug)]
pub enum DictionaryError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("FST error: {0}")]
    Fst(#[from] fst::Error),

    #[error("Invalid UTF-8 sequence: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),
}

/// Adapts a Levenshtein [`DFA`] to the [`fst::Automaton`] trait ecosystem.
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

/// Memory-mapped FST dictionary for fuzzy string lookups.
pub struct Dictionary {
    map: Set<Mmap>,
}

/// A matched item from a fuzzy search.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SearchResult {
    /// True if Levenshtein distance is 0.
    pub is_exact: bool,
    /// The matched string.
    pub key: String,
    /// Levenshtein distance to the query.
    pub distance: u8,
}

/// Query builder for configuring fuzzy searches.
pub struct SearchBuilder<'a> {
    dictionary: &'a Dictionary,
    query: String,
    limit: usize,
    distance: u8,
    transposition: bool,
}

impl<'a> SearchBuilder<'a> {
    /// Defaults: `limit = 5`, `distance = 1`, `transposition = false`.
    pub fn new(dictionary: &'a Dictionary, query: &str) -> Self {
        Self {
            dictionary,
            query: query.to_string(),
            limit: 5,
            distance: 1,
            transposition: false,
        }
    }

    /// Max number of results to return.
    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }

    /// Sets the maximum Levenshtein distance for fuzzy searching (hard-capped at 2).
    pub fn distance(mut self, distance: u8) -> Self {
        self.distance = distance.min(2);
        self
    }

    /// Sets whether to allow transpositions (e.g., swapping adjacent characters like "teh" -> "the").
    pub fn transposition(mut self, transposition: bool) -> Self {
        self.transposition = transposition;
        self
    }

    /// Evaluates the fuzzy search against the FST.
    pub fn execute(self) -> Result<Vec<SearchResult>, DictionaryError> {
        let builder = LevenshteinAutomatonBuilder::new(self.distance, self.transposition);
        let dfa = FstDfaWrapper(builder.build_dfa(&self.query));

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
                    distance: dist,
                })
            })
            .collect::<Result<_, DictionaryError>>()?;

        results
            .sort_unstable_by(|a, b| a.distance.cmp(&b.distance).then_with(|| a.key.cmp(&b.key)));

        Ok(results)
    }
}

impl Dictionary {
    /// Memory-maps an existing compiled FST file.
    ///
    /// # Examples
    /// ```no_run
    /// # use fuzzies::{Dictionary, DictionaryError};
    /// # fn main() -> Result<(), DictionaryError> {
    /// let dict = Dictionary::open("dict.fst")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DictionaryError> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };
        let map = Set::new(mmap)?;

        Ok(Self { map })
    }

    /// Sorts a newline-delimited text file in-place by byte order.
    ///
    /// Prepares raw source text for processing by [`Self::build`].
    ///
    /// # Examples
    /// ```no_run
    /// # use fuzzies::{Dictionary, DictionaryError};
    /// # fn main() -> Result<(), DictionaryError> {
    /// Dictionary::sort("unsorted_words.txt")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Warning
    /// Loads the entire file into memory. Use an external CLI utility like `sort` for massive datasets.
    pub fn sort(path: impl AsRef<Path>) -> Result<(), DictionaryError> {
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

    /// Compiles a byte-sorted text file into an immutable binary FST.
    pub fn build(
        input_path: impl AsRef<Path>,
        output_path: impl AsRef<Path>,
    ) -> Result<(), DictionaryError> {
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

    /// Initializes a fuzzy search query builder.
    ///
    /// # Examples
    /// ```no_run
    /// # use fuzzies::{Dictionary, DictionaryError};
    /// # fn main() -> Result<(), DictionaryError> {
    /// # let dict = Dictionary::open("dict.fst")?;
    /// let results = dict.search("baxana")
    ///     .distance(2)
    ///     .limit(5)
    ///     .execute()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn search<'a>(&'a self, query: &str) -> SearchBuilder<'a> {
        SearchBuilder::new(self, query)
    }

    /// Executes multiple search queries concurrently via Rayon.
    ///
    /// # Examples
    /// ```no_run
    /// # use fuzzies::{Dictionary, DictionaryError};
    /// # fn main() -> Result<(), DictionaryError> {
    /// # let dict = Dictionary::open("dict.fst")?;
    /// let batch_results = dict.batch_search(&["baxana", "appl", "cheriy"]);
    /// # Ok(())
    /// # }
    /// ```
    pub fn batch_search(
        &self,
        queries: &[&str],
    ) -> Vec<Result<Vec<SearchResult>, DictionaryError>> {
        queries
            .par_iter()
            .map(|&query| self.search(query).execute())
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

        Dictionary {
            map: Set::new(mmap).unwrap(),
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

        // Test transpositiom
        let results = dict
            .search("banaan")
            .limit(1)
            .transposition(true)
            .execute()
            .unwrap();
        let keys: Vec<&str> = results.iter().map(|r| r.key.as_ref()).collect();
        assert_eq!(keys, vec!["banana"]);
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
        let input_words = vec!["ßé", "àé", "🤣"];
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
            vec!["ßé".to_string(), "àé".to_string(), "🤣".to_string(),]
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

        let results = dict.search("test").distance(2).limit(2).execute().unwrap();
        let keys: Vec<&str> = results.iter().map(|r| r.key.as_ref()).collect();

        // "fest" should come first a distance of 1 is a better match than 2.
        assert_eq!(keys, vec!["fest", "east"]);
    }
}
