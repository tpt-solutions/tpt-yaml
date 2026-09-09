#![no_main]

use libfuzzer_sys::fuzz_target;
use tpt_yaml_serde::Value;

fuzz_target!(|data: &[u8]| {
    let _ = tpt_yaml_serde::from_slice::<Value>(data);
});
