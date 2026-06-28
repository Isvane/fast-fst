# Fuzzies

Fuzzy search crate for Rust.

[![Crates.io](https://img.shields.io/crates/v/fuzzies.svg)](https://crates.io/crates/fuzzies)
[![Docs.rs](https://docs.rs/fuzzies/badge.svg)](https://docs.rs/fuzzies)
[![Crates.io](https://img.shields.io/crates/l/fuzzies)](https://github.com/Isvane/fuzzies/blob/main/LICENSE)

More information about this crate can be found in the [crate documentation](https://docs.rs/fuzzies)

> [!WARNING]  
> This library is a student learning project in early development. Breaking changes may occur frequently and without warning.

---

## Installation

```bash
cargo add fuzzies
```

---

## Example

This library allows you to build a compact, memory-mapped FST from a file and perform fast, fuzzy searches with configurable Levenshtein distances.

```rust, no_run
use fuzzies::Dictionary;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Prepare your raw text file (must be sorted lexicographically)
    // Fuzzies provides a handy in-place sorter for convenience:
    Dictionary::sort("words.txt")?;

    // 2. Build the immutable binary FST from the sorted text file
    Dictionary::build("words.txt", "words.fst")?;

    // 3. Load the dictionary
    let dict = Dictionary::open("words.fst")?;

    // 4. Perform a fuzzy search with a max typo distance of 2 and limit of 5 results
    // We can also enable transposition handling (e.g., "banaan" -> "banana")
    let results = dict.search("banaan")
        .distance(2)
        .transposition(true)
        .limit(5)
        .execute()?;
    
    for result in results {
        println!("Found: {} (Distance: {}, Exact: {})", result.key, result.distance, result.is_exact);
    }

    // 5. Batch search (multithreaded, defaults to a distance of 1)
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

---

## Performance

The following benchmarks were gathered using Criterion to evaluate lookup speeds for single and parallel batch searches.
You can re-run these benchmarks on your hardware using `cargo bench`.

### Single Search

```ignore
Dictionary Single Search/apple          6.8904 µs/iter (+/- 0.0174 µs)
Dictionary Single Search/baxana         8.1007 µs/iter (+/- 0.0321 µs)
Dictionary Single Search/missingword   12.1830 µs/iter (+/- 0.0285 µs)
```

### Batch Search

```ignore
Rayon Parallel Batch/100 queries      406.79 µs/iter (+/- 1.60 µs)
Rayon Parallel Batch/500 queries     1.9530 ms/iter (+/- 0.0051 ms)
Rayon Parallel Batch/1000 queries    3.9583 ms/iter (+/- 0.0141 ms)
```

> [!NOTE]
> Benchmarks were executed on an Intel Core i5-10300H (4 cores, 8 threads, Battery set to High Performance mode). Performance may scale significantly higher on more modern or high-end CPUs.

---

## License

This project is licensed under the [MIT license.](LICENSE)
