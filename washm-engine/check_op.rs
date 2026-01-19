fn main() {
    let op: wasmparser::Operator = unsafe { std::mem::zeroed() };
    match op {
        wasmparser::Operator::NonExistentVariant => {},
        _ => {}
    }
}
