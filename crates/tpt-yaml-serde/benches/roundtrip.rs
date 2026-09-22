//! `from_str`/`to_string` round-trip benchmarks, head-to-head against `serde_yaml`.
//!
//! Run with `cargo bench -p tpt-yaml-serde`; the HTML report lands at
//! `target/criterion/report/index.html`.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use serde::{Deserialize, Serialize};

/// A representative "record" shape: scalar fields plus a nested struct and a
/// list of children, similar to what a config or API payload looks like.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Address {
    street: String,
    city: String,
    zip: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Person {
    id: u64,
    name: String,
    email: String,
    active: bool,
    score: f64,
    address: Address,
    tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Roster {
    org: String,
    people: Vec<Person>,
}

fn make_person(i: u64) -> Person {
    Person {
        id: i,
        name: format!("Person {i}"),
        email: format!("person{i}@example.com"),
        active: i % 2 == 0,
        score: i as f64 * 1.5,
        address: Address {
            street: format!("{i} Main St"),
            city: "Springfield".to_string(),
            zip: format!("{:05}", i % 100000),
        },
        tags: vec!["a".to_string(), "b".to_string(), "c".to_string()],
    }
}

fn make_roster(len: u64) -> Roster {
    Roster { org: "Acme".to_string(), people: (0..len).map(make_person).collect() }
}

fn bench_roundtrip(c: &mut Criterion) {
    let sizes: [u64; 3] = [1, 20, 200];

    let mut ser_group = c.benchmark_group("serialize");
    for &size in &sizes {
        let roster = make_roster(size);
        ser_group.bench_with_input(
            BenchmarkId::new("tpt_yaml_serde", size),
            &roster,
            |b, roster| {
                b.iter(|| tpt_yaml_serde::to_string(roster).unwrap());
            },
        );
        ser_group.bench_with_input(BenchmarkId::new("serde_yaml", size), &roster, |b, roster| {
            b.iter(|| serde_yaml::to_string(roster).unwrap());
        });
    }
    ser_group.finish();

    let mut de_group = c.benchmark_group("deserialize");
    for &size in &sizes {
        let roster = make_roster(size);
        let yaml = tpt_yaml_serde::to_string(&roster).unwrap();
        de_group.bench_with_input(BenchmarkId::new("tpt_yaml_serde", size), &yaml, |b, yaml| {
            b.iter(|| tpt_yaml_serde::from_str::<Roster>(yaml).unwrap());
        });
        de_group.bench_with_input(BenchmarkId::new("serde_yaml", size), &yaml, |b, yaml| {
            b.iter(|| serde_yaml::from_str::<Roster>(yaml).unwrap());
        });
    }
    de_group.finish();

    let mut rt_group = c.benchmark_group("roundtrip");
    for &size in &sizes {
        let roster = make_roster(size);
        rt_group.bench_with_input(
            BenchmarkId::new("tpt_yaml_serde", size),
            &roster,
            |b, roster| {
                b.iter(|| {
                    let s = tpt_yaml_serde::to_string(roster).unwrap();
                    tpt_yaml_serde::from_str::<Roster>(&s).unwrap()
                });
            },
        );
        rt_group.bench_with_input(BenchmarkId::new("serde_yaml", size), &roster, |b, roster| {
            b.iter(|| {
                let s = serde_yaml::to_string(roster).unwrap();
                serde_yaml::from_str::<Roster>(&s).unwrap()
            });
        });
    }
    rt_group.finish();
}

criterion_group!(benches, bench_roundtrip);
criterion_main!(benches);
