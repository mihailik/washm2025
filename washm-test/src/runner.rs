use serde::Deserialize;
use std::collections::BTreeMap;
use std::env;
use std::path::PathBuf;
use std::process::Command;
use wasi_common::sync::WasiCtxBuilder;
use wasmtime::*;

#[derive(Deserialize)]
struct Sidecar {
    #[allow(dead_code)]
    module_id: String,
    probe_map: BTreeMap<usize, (String, u64)>,
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        anyhow::bail!("Usage: washm-run <wasm-binary>");
    }

    let wasm_binary = &args[1];
    let pass_args = &args[2..];

    // Call washm-distill
    // We pass the wasm binary to it so it generates .distilled.wasm and JSON map
    let status = Command::new("cargo")
        .args([
            "run",
            "-p",
            "washm-test",
            "--bin",
            "washm-distill",
            "--",
            wasm_binary,
        ])
        .status()?;

    if !status.success() {
        anyhow::bail!("washm-distill failed");
    }

    let mut distilled_path = PathBuf::from(wasm_binary);
    distilled_path.set_extension("distilled.wasm");

    let mut sidecar_path = PathBuf::from(wasm_binary);
    sidecar_path.set_extension("distilled.json");

    let sidecar_json = std::fs::read_to_string(&sidecar_path)?;
    let sidecar: Sidecar = serde_json::from_str(&sidecar_json)?;

    let distilled_bytes = std::fs::read(&distilled_path)?;

    // 1. Setup Wasmtime
    let engine = Engine::default();
    let mut linker = Linker::new(&engine);
    wasi_common::sync::add_to_linker(&mut linker, |s| s)?;

    let mut wasi_args = vec!["washm-guest".to_string()];
    wasi_args.extend_from_slice(pass_args);

    let wasi = WasiCtxBuilder::new()
        .inherit_stdout()
        .inherit_stderr()
        .args(&wasi_args)?
        .build();
    let mut store = Store::new(&engine, wasi);

    // 2. Load Module from memory
    let module = Module::new(&engine, &distilled_bytes)?;
    let instance = linker.instantiate(&mut store, &module)?;

    // 3. Detect DMZ
    let cov_ptr_global = instance
        .get_global(&mut store, "__WASHM_COV_PTR")
        .ok_or_else(|| anyhow::anyhow!("__WASHM_COV_PTR not exported"))?;
    let dmz_addr = cov_ptr_global.get(&mut store).i32().unwrap() as u32;

    // 4. Run (WASI usually uses _start for commands)
    let main_func = instance.get_typed_func::<(), ()>(&mut store, "_start")?;
    let run_res = main_func.call(&mut store, ());

    let run_passed = run_res.is_ok();

    // 5. Collect
    let memory = instance.get_memory(&mut store, "memory").unwrap();
    let max_probe_id = sidecar.probe_map.keys().max().copied().unwrap_or(0);
    let num_probes = max_probe_id + 1;
    let mut coverage_buffer = vec![0u8; num_probes];
    memory.read(&store, dmz_addr as usize, &mut coverage_buffer)?;

    let mut hit_count = 0;

    let mut lcov_output = String::new();

    // Group hits by file for LCOV
    let mut file_hits: BTreeMap<String, BTreeMap<u64, u8>> = BTreeMap::new();

    for (probe_id, &(ref filename, line)) in &sidecar.probe_map {
        let hits = coverage_buffer[*probe_id];
        if hits > 0 {
            hit_count += 1;
        }
        file_hits
            .entry(filename.clone())
            .or_default()
            .insert(line, hits);
    }

    for (file, lines) in file_hits {
        lcov_output.push_str(&format!("SF:{}\n", file));
        let mut lf = 0;
        let mut lh = 0;
        for (line, hits) in lines {
            lf += 1;
            if hits > 0 {
                lh += 1;
            }
            lcov_output.push_str(&format!("DA:{},{}\n", line, hits));
        }
        lcov_output.push_str(&format!("LF:{}\n", lf));
        lcov_output.push_str(&format!("LH:{}\n", lh));
        lcov_output.push_str("end_of_record\n");
    }

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_micros();
    let lcov_path = format!("lcov_{}.info", timestamp);
    std::fs::write(&lcov_path, lcov_output)?;

    println!(
        "washm-run: Coverage: {}/{} hit ({:.2}%) -> {}",
        hit_count,
        num_probes,
        (hit_count as f64 / num_probes as f64) * 100.0,
        lcov_path
    );

    if !run_passed {
        if let Err(e) = run_res {
            eprintln!("Guest Execution Failed: {:?}", e);
        }
        std::process::exit(1);
    }

    Ok(())
}
