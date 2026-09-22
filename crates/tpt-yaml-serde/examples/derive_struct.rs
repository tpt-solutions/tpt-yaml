//! Round-trip a `#[derive(Serialize, Deserialize)]` struct through `tpt-yaml-serde`.
//!
//! Run with: `cargo run --example derive_struct -p tpt-yaml-serde`

use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Config {
    name: String,
    retries: u32,
    tags: Vec<String>,
}

fn main() {
    let source = "name: demo\nretries: 3\ntags:\n  - a\n  - b\n";

    let config: Config = tpt_yaml_serde::from_str(source).expect("valid YAML for Config");
    println!("deserialized: {config:?}");

    let rendered = tpt_yaml_serde::to_string(&config).expect("Config serializes");
    println!("re-serialized:\n{rendered}");

    let round_tripped: Config =
        tpt_yaml_serde::from_str(&rendered).expect("re-serialized YAML parses back");
    assert_eq!(config, round_tripped);
    println!("round-trip OK");
}
