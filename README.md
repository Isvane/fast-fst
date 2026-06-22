# Fuzzies

A high-performance fuzzy search library for Rust, leveraging Finite State Transducers (FST) and Levenshtein Automata for efficient dictionary lookups.

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
    let results = dict.search("aple")?;
    
    for result in results {
        println!("Found: {} (Exact: {})", result.key, result.is_exact);
    }

    // 4. Batch search (multithreaded)
    let queries = vec!["aple", "baxana", "cherri"];
    let batch_results = dict.batch_search(&queries);

    Ok(())
}
```
