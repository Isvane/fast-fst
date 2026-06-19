use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter};
use std::path::Path;

use fst::SetBuilder;

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
