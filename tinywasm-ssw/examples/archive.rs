use eyre::Result;
use tinywasm::{Module, Store, parser::Parser, types::TinyWasmModule};

const WASM: &str = r#"
(module
  (func $add (param $lhs i32) (param $rhs i32) (result i32)
    local.get $lhs
    local.get $rhs
    i32.add)
  (export "add" (func $add)))
"#;

fn main() -> Result<()> {
    let lexer = wast::lexer::Lexer::new(WASM);
    let buf = wast::parser::ParseBuffer::new_with_lexer(lexer).expect("failed to create parse buffer");
    let mut wat_data = wast::parser::parse::<wast::Wat>(&buf).expect("failed to parse wat");
    let wasm = wat_data.encode().expect("failed to encode module");
    let module = Parser::default().parse_module_bytes(wasm)?;
    let twasm = module.serialize_twasm()?;

    // Now, you could e.g. write `twasm` to a file called `add.twasm`
    // and load it later in a different program.

    let module: Module = TinyWasmModule::from_twasm(&twasm)?.into();
    let mut store = Store::default();
    let instance = module.instantiate(&mut store, None)?;
    let add = instance.exported_func::<(i32, i32), i32>(&store, "add")?;

    assert_eq!(add.call(&mut store, (1, 2))?, 3);

    Ok(())
}
