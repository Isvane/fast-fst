#![doc = include_str!("../README.md")]

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::error::Error;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter};

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
    pub lev_builders: Vec<LevenshteinAutomatonBuilder>,
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
    distance: u8,
}

impl<'a> SearchBuilder<'a> {
    pub fn new(dictionary: &'a Dictionary, query: &str) -> Self {
        Self {
            dictionary,
            query: query.to_string(),
            limit: 5,
            distance: 1,
        }
    }

    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }

    pub fn distance(mut self, distance: u8) -> Self {
        self.distance = distance;
        self
    }

    pub fn execute(self) -> Result<Vec<SearchResult>, Box<dyn Error>> {
        let dfa = if let Some(builder) = self.dictionary.lev_builders.get(self.distance as usize) {
            FstDfaWrapper(builder.build_dfa(&self.query))
        } else {
            let builder = LevenshteinAutomatonBuilder::new(self.distance, false);
            FstDfaWrapper(builder.build_dfa(&self.query))
        };

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
    pub fn open(path: &str) -> Result<Self, Box<dyn Error>> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };
        let map = Set::new(mmap)?;

        let lev_builders = (0..=2)
            .map(|d| LevenshteinAutomatonBuilder::new(d, false))
            .collect();

        Ok(Self { map, lev_builders })
    }

    pub fn build(input_path: &str, output_path: &str) -> Result<(), Box<dyn Error>> {
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
}
