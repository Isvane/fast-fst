#![doc = include_str!("../README.md")]

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::error::Error;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter};
use std::path::Path;

use fst::{IntoStreamer, Set, SetBuilder, Streamer};
use levenshtein_automata::{DFA, Distance, LevenshteinAutomatonBuilder};
use memmap2::Mmap;
use rayon::prelude::*;

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
}

pub struct Dictionary {
    pub map: Set<Mmap>,
    pub lev_builder: LevenshteinAutomatonBuilder,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SearchResult {
    pub is_exact: bool,
    pub key: String,
}

pub struct SearchBuilder<'a> {
    dictionary: &'a Dictionary,
    query: String,
    limit: usize,
}

impl<'a> SearchBuilder<'a> {
    pub fn new(dictionary: &'a Dictionary, query: &str) -> Self {
        Self {
            dictionary,
            query: query.to_string(),
            limit: 5,
        }
    }

    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }

    pub fn execute(self) -> Result<Vec<SearchResult>, Box<dyn Error>> {
        let dfa = FstDfaWrapper(self.dictionary.lev_builder.build_dfa(&self.query));
        let mut heap = BinaryHeap::with_capacity(self.limit);
        let query_bytes = self.query.as_bytes();

        let mut stream = self.dictionary.map.search(&dfa).into_stream();

        while let Some(key_bytes) = stream.next() {
            let is_exact = key_bytes == query_bytes;
            let candidate = (Reverse(is_exact), key_bytes);

            if heap.len() < self.limit {
                heap.push((Reverse(is_exact), key_bytes.to_vec()));
            } else if let Some(mut worst) = heap.peek_mut() {
                if candidate < (worst.0, worst.1.as_slice()) {
                    *worst = (Reverse(is_exact), key_bytes.to_vec());
                }
            }
        }

        let mut results: Vec<_> = heap
            .into_iter()
            .map(|(Reverse(is_exact), bytes)| {
                Ok(SearchResult {
                    is_exact,
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
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Box<dyn Error>> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };
        let map = Set::new(mmap)?;

        Ok(Self {
            map: map,
            lev_builder: LevenshteinAutomatonBuilder::new(1, false),
        })
    }

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

    pub fn search<'a>(&'a self, query: &str) -> SearchBuilder<'a> {
        SearchBuilder::new(self, query)
    }

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

        Dictionary {
            map: Set::new(mmap).unwrap(),
            lev_builder: LevenshteinAutomatonBuilder::new(1, false),
        }
    }

    #[test]
    fn test_dictionary_search() {
        let dict = create_test_dict(&["apple", "banana", "cherry", "lime", "time", "mime"]);

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
}
