# Fuzzies

Fuzzy search crate for Rust.

[![Crates.io](https://img.shields.io/crates/v/fuzzies.svg)](https://crates.io/crates/fuzzies)
[![Docs.rs](https://docs.rs/fuzzies/badge.svg)](https://docs.rs/fuzzies)
[![Crates.io](https://img.shields.io/crates/l/fuzzies)](LICENSE)

More information about this crate can be found in the [crate documentation](https://docs.rs/fuzzies)

> [!WARNING]  
> **Early Development & Disclaimer:** This project is in its early stages of development. **Breaking changes may occur frequently** and without warning between versions. This library is built by a university sophomore for personal learning and experimentation, not as a full-time, production-ready project. Use with caution!

## Installation

```bash
cargo add fuzzies
```

## Example

This library allows you to build a compact, memory-mapped FST from a file and perform fast, fuzzy searches (Levenshtein distance of 1).

```rust, no_run
use fuzzies::Dictionary;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Build the dictionary from a text file (one word per line)
    // Note: The input file must be sorted lexicographically
    fuzzies::build("words.txt", "words.fst")?;

    // 2. Load the dictionary
    let dict = Dictionary::open("words.fst")?;

    // 3. Perform a fuzzy search
    let results = dict.search("aple").execute()?;
    
    for result in results {
        println!("Found: {} (Exact: {})", result.key, result.is_exact);
    }

    // 4. Batch search (multithreaded, returns a Vec of Results)
    let queries = vec!["aple", "baxana", "cherri"];
    let batch_results = dict.batch_search(&queries);

    for (query, result) in queries.iter().zip(batch_results) {
        match result {
            Ok(matches) => println!("Query '{}' found {} matches", query, matches.len()),
            Err(e) => eprintln!("Error searching for '{}': {}", query, e),
        }
    }

    Ok(())
}
```
