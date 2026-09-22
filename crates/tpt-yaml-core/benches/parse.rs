//! Parse benchmarks for `tpt_yaml_core::parse`, head-to-head against `yaml-rust2`.
//!
//! Run with `cargo bench -p tpt-yaml-core`; the HTML report lands at
//! `target/criterion/report/index.html`.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};

/// Small, flat mapping — the common "config file" shape.
fn small_flat_mapping() -> String {
    let mut s = String::new();
    for i in 0..20 {
        s.push_str(&format!("key_{i}: value number {i}\n"));
    }
    s
}

/// Deeply nested mapping, one key per level.
fn deeply_nested(depth: usize) -> String {
    let mut s = String::new();
    for i in 0..depth {
        s.push_str(&"  ".repeat(i));
        s.push_str(&format!("level_{i}:\n"));
    }
    s.push_str(&"  ".repeat(depth));
    s.push_str("leaf: true\n");
    s
}

/// Large flat sequence of scalars.
fn large_sequence(len: usize) -> String {
    let mut s = String::new();
    for i in 0..len {
        s.push_str(&format!("- item_{i}\n"));
    }
    s
}

/// Anchors/aliases-heavy document: a handful of anchored mappings referenced
/// repeatedly via aliases, plus a merge key.
fn anchors_and_aliases(reps: usize) -> String {
    let mut s = String::new();
    s.push_str("base: &base\n  a: 1\n  b: 2\n  c: 3\n");
    s.push_str("items:\n");
    for i in 0..reps {
        s.push_str(&format!("  - <<: *base\n    id: {i}\n"));
    }
    s
}

fn bench_parse(c: &mut Criterion) {
    let docs: Vec<(&str, String)> = vec![
        ("small_flat_mapping", small_flat_mapping()),
        ("deeply_nested_64", deeply_nested(64)),
        ("large_sequence_2000", large_sequence(2000)),
        ("anchors_aliases_200", anchors_and_aliases(200)),
    ];

    let mut group = c.benchmark_group("parse");
    for (name, doc) in &docs {
        group.bench_with_input(BenchmarkId::new("tpt_yaml_core", name), doc, |b, doc| {
            b.iter(|| tpt_yaml_core::parse(doc).unwrap());
        });
        group.bench_with_input(BenchmarkId::new("yaml_rust2", name), doc, |b, doc| {
            b.iter(|| yaml_rust2::YamlLoader::load_from_str(doc).unwrap());
        });
    }
    group.finish();
}

criterion_group!(benches, bench_parse);
criterion_main!(benches);
