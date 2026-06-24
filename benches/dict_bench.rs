use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use fuzzies::{Dictionary, SearchResult};
use std::hint::black_box;
use std::time::Duration;

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

    // --- Single Search Group ---
    let mut single_group = c.benchmark_group("Dictionary Single Search");
    let queries = vec!["apple", "baxana", "missingword"];
    for query in queries {
        single_group.bench_with_input(BenchmarkId::from_parameter(query), query, |b, q| {
            b.iter(|| {
                let _res: Vec<SearchResult> =
                    black_box(dict.search(black_box(q)).execute().unwrap());
            });
        });
    }
    single_group.finish();

    // --- Batch Search Group (100, 500, 1000) ---
    let mut batch_group = c.benchmark_group("Dictionary Batch Search");

    let batch_sizes = vec![100, 500, 1000];

    for size in batch_sizes {
        // Generate the queries dynamically based on the current batch size
        let batch_queries: Vec<&str> = (0..size)
            .map(|i| if i % 2 == 0 { "word0050" } else { "baxana" })
            .collect();

        batch_group.bench_with_input(
            BenchmarkId::new("Rayon Parallel Batch", size),
            &batch_queries,
            |b, queries| {
                b.iter(|| {
                    let _res = black_box(dict.batch_search(black_box(queries)));
                });
            },
        );
    }
    batch_group.finish();
}

fn configured_criterion() -> Criterion {
    Criterion::default().measurement_time(Duration::from_secs(10))
}

criterion_group!(
    name = benches;
    config = configured_criterion();
    targets = bench_searches
);
criterion_main!(benches);
