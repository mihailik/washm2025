mod common;

#[test]
fn trace_basic() {
    let outs = common::trace_lifter_passes_for_source("tests/roundtrip/_basic.rs");
    for (label, out) in outs {
        println!("--- PASS: {} ---\n{}\n", label, out);
    }
}
