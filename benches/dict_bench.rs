use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use fuzzies::{Dictionary, SearchResult};
use std::hint::black_box;

fn setup_bench_dictionary() -> (Dictionary, tempfile::NamedTempFile) {
    use fst::SetBuilder;

    let mut temp_file = tempfile::NamedTempFile::new().unwrap();

    let mut words = vec![
        "apple".to_string(),
        "banana".to_string(),
        "cherry".to_string(),
        "date".to_string(),
        "fig".to_string(),
        "grape".to_string(),
    ];

    for i in 0..1000 {
        words.push(format!("word{:04}", i));
    }
    words.sort_unstable();

    let mut build = SetBuilder::new(&mut temp_file).unwrap();
    for word in words {
        build.insert(word).unwrap();
    }
    build.finish().unwrap();

    let dict = Dictionary::open(temp_file.path().to_str().unwrap()).unwrap();
    (dict, temp_file)
}

fn bench_searches(c: &mut Criterion) {
    let (dict, _temp) = setup_bench_dictionary();

    let mut group = c.benchmark_group("Dictionary Single Search");

    let queries = vec!["apple", "baxana", "missingword"];
    for query in queries {
        group.bench_with_input(BenchmarkId::from_parameter(query), query, |b, q| {
            b.iter(|| {
                let _res: Vec<SearchResult> =
                    black_box(dict.search(black_box(q)).execute().unwrap());
            });
        });
    }
    group.finish();

    let mut batch_group = c.benchmark_group("Dictionary Batch Search");

    let batch_queries: Vec<&str> = (0..500)
        .map(|i| if i % 2 == 0 { "word0050" } else { "baxana" })
        .collect();

    batch_group.bench_function("Rayon Parallel Batch (500 queries)", |b| {
        b.iter(|| {
            let _res = black_box(dict.batch_search(black_box(&batch_queries)));
        });
    });
    batch_group.finish();
}

criterion_group!(benches, bench_searches);
criterion_main!(benches);
