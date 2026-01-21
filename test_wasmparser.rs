
use wasmparser::GlobalType;
fn main() {
    let gt = GlobalType {
        content_type: wasmparser::ValType::I32,
        mutable: true,
        shared: false,
    };
}
