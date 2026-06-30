#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::{BufRead, BufReader, Write};

    use crate::Dictionary;
    use fst::{Set, SetBuilder};

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
