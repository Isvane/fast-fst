use std::cmp::Ordering;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter};

use fst::automaton::Levenshtein;
use fst::{IntoStreamer, Set, SetBuilder, Streamer};
use memmap2::Mmap;
use rayon::prelude::*;

pub fn build(input_path: &str, output_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::open(input_path)?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();

    let writer = BufWriter::new(File::create(output_path)?);
    let mut build = SetBuilder::new(writer)?;

    while reader.read_line(&mut line)? > 0 {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            line.clear();
            continue;
        }

        build.insert(trimmed)?;
        line.clear();
    }

    build.finish()?;
    Ok(())
}

pub struct Dictionary {
    pub map: Set<Mmap>,
}

#[derive(Eq)]
pub struct SearchResult {
    pub is_exact: bool,
    pub key: String,
}

impl SearchResult {
    fn priority_key(&self) -> impl Ord + '_ {
        (Reverse(self.is_exact), &self.key)
    }
}

impl Ord for SearchResult {
    fn cmp(&self, other: &Self) -> Ordering {
        self.priority_key().cmp(&other.priority_key())
    }
}

impl PartialOrd for SearchResult {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for SearchResult {
    fn eq(&self, other: &Self) -> bool {
        self.priority_key() == other.priority_key()
    }
}

#[allow(dead_code)]
impl Dictionary {
    pub fn open(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let data = File::open(path)?;
        let mmap = unsafe { Mmap::map(&data)? };
        let map = Set::new(mmap)?;
        Ok(Self { map })
    }

    pub fn search<'a>(
        &'a self,
        query: &str,
    ) -> Result<Vec<SearchResult>, Box<dyn std::error::Error>> {
        let search_term = if query.is_empty() { "" } else { query };
        let lev = Levenshtein::new(search_term, 1)?;

        let mut heap = BinaryHeap::with_capacity(5);

        let mut stream = self.map.search(lev).into_stream();

        while let Some(key_bytes) = stream.next() {
            let key = std::str::from_utf8(key_bytes)?.to_string();
            let is_exact = key == search_term;

            let result = SearchResult { is_exact, key };

            if heap.len() < 5 {
                heap.push(result);
            } else if let Some(mut worst_of_the_best) = heap.peek_mut() {
                if result < *worst_of_the_best {
                    *worst_of_the_best = result;
                }
            }
        }

        Ok(heap.into_sorted_vec())
    }

    pub fn batch_search(
        &self,
        queries: &[&str],
    ) -> Vec<Result<Vec<SearchResult>, Box<dyn std::error::Error + Send + Sync>>> {
        queries
            .par_iter()
            .map(|&query| {
                self.search(query)
                    .map_err(|e| format!("Search failed: {}", e).into())
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

        Dictionary {
            map: Set::new(mmap).unwrap(),
        }
    }

    #[test]
    fn test_dictionary_search() {
        let dict = create_test_dict(&["apple", "banana", "cherry"]);

        // Exact match
        let results = dict.search("apple").unwrap();
        let keys: Vec<&str> = results.iter().map(|r| r.key.as_ref()).collect();
        assert_eq!(keys, vec!["apple"]);

        // Levenshtein distance of 1 (substitution)
        let results = dict.search("baxana").unwrap();
        let keys: Vec<&str> = results.iter().map(|r| r.key.as_ref()).collect();
        assert_eq!(keys, vec!["banana"]);

        // Levenshtein distance of 1 (deletion/substitution)
        let results = dict.search("cheriy").unwrap();
        let keys: Vec<&str> = results.iter().map(|r| r.key.as_ref()).collect();
        assert_eq!(keys, vec!["cherry"]);

        // Out of bounds (> 1 distance)
        let results = dict.search("ap").unwrap();
        assert!(results.is_empty());
    }
}
