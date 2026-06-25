# Fuzzies

Fuzzy search crate for Rust.

[![Crates.io](https://img.shields.io/crates/v/fuzzies.svg)](https://crates.io/crates/fuzzies)
[![Docs.rs](https://docs.rs/fuzzies/badge.svg)](https://docs.rs/fuzzies)
[![Crates.io](https://img.shields.io/crates/l/fuzzies)](https://github.com/Isvane/fuzzies/blob/main/LICENSE)

More information about this crate can be found in the [crate documentation](https://docs.rs/fuzzies)

> [!WARNING]  
> **Early Development & Disclaimer:** This project is in its early stages of development. **Breaking changes may occur frequently** and without warning between versions. This library is built by a university sophomore for personal learning and experimentation, not as a full-time, production-ready project. Use with caution!

## Installation

```bash
cargo add fuzzies
```

## Example

This library allows you to build a compact, memory-mapped FST from a file and perform fast, fuzzy searches with configurable Levenshtein distances (supporting distances of 1 and 2).

```rust, no_run
use fuzzies::Dictionary;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Build the dictionary from a text file (one word per line)
    // Note: The input file must be sorted lexicographically
    Dictionary::build("words.txt", "words.fst")?;

    // 2. Load the dictionary
    let dict = Dictionary::open("words.fst")?;

    // 3. Perform a fuzzy search with a max typo distance of 2 and limit of 5 results
    let results = dict.search("baxaxa")
        .distance(2)
        .limit(5)
        .execute()?;
    
    for result in results {
        println!("Found: {} (Exact: {})", result.key, result.is_exact);
    }

    // 4. Batch search (multithreaded, defaults to distance of 1)
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

## Performance

The following benchmarks were gathered using Criterion to evaluate lookup speeds for single and parallel batch searches.
You can re-run these benchmarks on your hardware using `cargo bench`.

## Single Search Performance

| Query                      | Execution Time (Avg) |
|---------------------------|----------------------|
| apple (Exact/Close match) | ~3.87 µs             |
| baxana (Fuzzy match)      | ~4.85 µs             |
| missingword (No match)    | ~8.93 µs             |

## Batch Search Performance

| Batch Size  | Total Execution Time | Per-Query Avg |
|------------|----------------------|---------------|
| 100 queries | ~302.6 µs           | ~3.02 µs      |
| 500 queries | ~1.42 ms            | ~2.84 µs      |
| 1000 queries| ~2.84 ms            | ~2.84 µs      |

*Note: Benchmarks were executed on an **Intel Core i5-10300H (4 cores, 8 threads, Battery set to HIgh Performance mode)**. Performance may scale significantly higher on more modern or high-end desktop/server CPUs.*

## License

This project is licensed under the [MIT license.](LICENSE)
