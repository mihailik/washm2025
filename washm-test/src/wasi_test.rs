use wasi_common::sync::WasiCtxBuilder;
use wasmtime::*;

fn main() -> anyhow::Result<()> {
    let mut config = Config::new();
    config.wasm_multi_memory(true);
    let engine = Engine::new(&config)?;
    let mut linker = Linker::new(&engine);
    wasi_common::sync::add_to_linker(&mut linker, |s| s)?;

    let wasi = WasiCtxBuilder::new()
        .inherit_stdout()
        .inherit_stderr()
        .build();
    let mut store = Store::new(&engine, wasi);

    let wasm_bytes = std::fs::read(
        "target/wasm32-wasip1/debug/deps/washm_test_guest-a7a2486ed4064207.distilled.wasm",
    )?;
    let module = Module::new(&engine, wasm_bytes)?;
    let instance = linker.instantiate(&mut store, &module)?;

    let main_func = instance.get_typed_func::<(), ()>(&mut store, "_start")?;
    let res = main_func.call(&mut store, ());
    println!("call returned {:?}", res);
    Ok(())
}
