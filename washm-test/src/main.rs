use clap::Parser;
use std::path::PathBuf;

mod instrument;
mod mapper;
mod shifter;

use instrument::Instrumenter;
use mapper::SourceMapper;
use shifter::FrontierShifter;

#[derive(Parser)]
#[command(
    author,
    version,
    about = "WASHM Test Distiller - Self-instrumenting WASM binaries"
)]
struct Args {
    /// Input .wasm file
    input: Option<PathBuf>,

    /// Output .distilled.wasm file
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Sidecar JSON mapping binary offsets to source locations
    #[arg(short, long)]
    mapping: Option<PathBuf>,

    /// Auto-build the washm-test-guest package
    #[arg(long)]
    build_guest: bool,
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let mut args = Args::parse();

    if args.build_guest {
        println!("Auto-building washm-test-guest...");
        let status = std::process::Command::new("powershell")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-File")
            .arg("./build-guest.ps1")
            .status()?;

        if !status.success() {
            anyhow::bail!("Failed to build guest via script");
        }

        // Update input path if not provided
        if args.input.is_none() {
            args.input = Some(PathBuf::from(
                "target/wasm32-wasip1/debug/washm_test_guest.wasm",
            ));
        }
    }

    let input_path = args.input.ok_or_else(|| {
        anyhow::anyhow!("No input file provided. Use --build-guest or specify <INPUT>")
    })?;

    println!("Distilling {}...", input_path.display());

    let bytes = std::fs::read(&input_path)?;
    println!("Main bytes address: {:p}", bytes.as_ptr());
    println!("File size: {} bytes", bytes.len());
    if bytes.len() >= 8 {
        println!(
            "Header: {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X}",
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7]
        );
    }

    // DEBUG: Try manual header check
    let expected_magic = [0x00, 0x61, 0x73, 0x6C]; // WRONG ON PURPOSE
    if bytes.starts_with(&expected_magic) {
        println!("Manual check: Magic matches \x00asl (unexpected!)");
    } else {
        println!("Manual check: Magic DOES NOT match \x00asl (expected)");
    }

    let actual_magic = [0x00, 0x61, 0x73, 0x6D];
    if bytes.starts_with(&actual_magic) {
        println!("Manual check: Magic matches \x00asm (expected)");
    } else {
        println!("Manual check: Magic DOES NOT match \x00asm (WTF!)");
    }

    // DUMP BYTES TO FILE FOR VERIFICATION
    std::fs::write("distiller_input_dump.wasm", &bytes)?;

    // Check with wasmparser to see what's actually in there
    // Calculate code section offset to adjust DWARF DWARF line info
    let mut code_offset = 0;
    let parser = wasmparser::Parser::new(0);
    for payload in parser.parse_all(&bytes) {
        match payload {
            Ok(wasmparser::Payload::Version { encoding, .. }) => {
                println!("wasmparser sees version, encoding: {:?}", encoding);
            }
            Ok(wasmparser::Payload::CodeSectionStart { range, .. }) => {
                code_offset = range.start as u32;
                println!("Code section starts at: 0x{:X}", code_offset);
            }
            Err(e) => {
                println!("wasmparser error: {}", e);
            }
            _ => {}
        }
    }

    println!("Attempting walrus::ModuleConfig::parse(bytes)...");
    let mut config = walrus::ModuleConfig::new();
    config.generate_dwarf(true);
    let mut module = config
        .parse(&bytes)
        .map_err(|e| anyhow::anyhow!("Walrus failed to parse WASM: {}", e))?;

    println!("Successfully parsed with Walrus!");

    // Phase 2: DWARF Loading
    let mut mapper = SourceMapper::new(&module, &bytes, code_offset)
        .map_err(|e| anyhow::anyhow!("SourceMapper failed: {}", e))?;
    println!("Successfully loaded DWARF mapping");

    // Phase 1: The Frontier Shifter
    let shifter = FrontierShifter::new(1024);
    let dmz_start = shifter.shift(&mut module)?;
    println!("Successfully shifted frontier");

    // Phase 3: Instrument
    let mut instrumenter = Instrumenter::new();
    instrumenter.find_and_patch_ptr(&mut module, dmz_start)?;
    let points = mapper.all_mapping_points();
    let probes = instrumenter.instrument(&mut module, &points)?;
    println!("Injected {} probes", probes);

    // Phase 3.5: Finalize Mapping
    if let Some(mid) = &instrumenter.module_id {
        mapper.module_id = mid.clone();
    }
    for (&probe_id, &offset) in instrumenter.get_probe_mapping() {
        mapper.populate_probes(probe_id, offset);
    }

    // Save output
    let output_path = args.output.clone().unwrap_or_else(|| {
        let mut p = input_path.clone();
        p.set_extension("distilled.wasm");
        p
    });

    module.emit_wasm_file(&output_path)?;
    println!("Emitted distilled binary to {}", output_path.display());

    // Phase 4: Emit Sidecar JSON
    let mapping_path = args.mapping.clone().unwrap_or_else(|| {
        let mut p = output_path.clone();
        p.set_extension("json");
        p
    });

    let sidecar_json = serde_json::to_string_pretty(&mapper)?;
    std::fs::write(&mapping_path, sidecar_json)?;
    println!("Emitted sidecar to {}", mapping_path.display());

    Ok(())
}
