use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter};
use std::path::Path;

use fst::automaton::Levenshtein;
use fst::{IntoStreamer, Set, SetBuilder};
use memmap2::Mmap;

struct Dictionary {
    map: Set<Mmap>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let txt_path = Path::new("dict.txt");
    let fst_path = Path::new("dict.fst");

    build(txt_path.to_str().unwrap(), fst_path.to_str().unwrap())?;

    Ok(())
}

fn build(input_path: &str, output_path: &str) -> Result<(), Box<dyn std::error::Error>> {
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

#[allow(dead_code)]
impl Dictionary {
    fn open(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let data = File::open(path)?;
        let mmap = unsafe { Mmap::map(&data)? };
        let map = Set::new(mmap)?;
        Ok(Self { map })
    }

    pub fn search<'a>(
        &'a self,
        query: &str,
    ) -> Result<fst::set::Stream<'a, Levenshtein>, Box<dyn std::error::Error>> {
        let search_term = if query.is_empty() { "" } else { query };
        let lev = Levenshtein::new(search_term, 1)?;

        Ok(self.map.search(lev).into_stream())
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
        let results = dict.search("apple").unwrap().into_strs().unwrap();
        assert_eq!(results, vec!["apple"]);

        // Levenshtein distance of 1 (substitution)
        let results = dict.search("baxana").unwrap().into_strs().unwrap();
        assert_eq!(results, vec!["banana"]);

        // Levenshtein distance of 1 (deletion/substitution)
        let results = dict.search("cheriy").unwrap().into_strs().unwrap();
        assert_eq!(results, vec!["cherry"]);

        // Out of bounds (> 1 distance)
        let results = dict.search("ap").unwrap().into_strs().unwrap();
        assert!(results.is_empty());
    }
}
