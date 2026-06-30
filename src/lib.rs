#![doc = include_str!("../README.md")]

use std::collections::BinaryHeap;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;

use fst::{Automaton, IntoStreamer, Set, SetBuilder, Streamer};
use levenshtein_automata::{DFA, Distance, LevenshteinAutomatonBuilder};
use memmap2::Mmap;
use rayon::prelude::*;

mod test;

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
    prefix: bool,
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
            prefix: false,
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

    /// Sets whether to perform a prefix fuzzy search.
    pub fn prefix(mut self, prefix: bool) -> Self {
        self.prefix = prefix;
        self
    }

    /// Evaluates the fuzzy search against the FST.
    pub fn execute(self) -> Result<Vec<SearchResult>, DictionaryError> {
        let builder = LevenshteinAutomatonBuilder::new(self.distance, self.transposition);

        let dfa = FstDfaWrapper(if self.prefix {
            builder.build_prefix_dfa(&self.query)
        } else {
            builder.build_dfa(&self.query)
        });

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
