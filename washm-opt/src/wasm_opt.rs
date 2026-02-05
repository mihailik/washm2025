use anyhow::{anyhow, Context};
use clap::{Arg, ArgAction, Command};
use std::path::PathBuf;
use washm::analysis::validation::Validator;
use washm::ir::module::Module;
use washm::passes::{OptimizationOptions, PassRunner, PASS_REGISTRY};
use washm::wasm::binary_reader::BinaryReader;
use washm::wasm::binary_writer::BinaryWriter;

mod tools;
use crate::tools::{add_feature_flags, apply_feature_flags, read_input, write_output};

fn main() -> anyhow::Result<()> {
    let cmd = Command::new("wasm-opt")
        .about("Optimizes WebAssembly files")
        .arg(
            Arg::new("input")
                .help("Input file (.wasm or .wat)")
                .required(true),
        )
        .arg(
            Arg::new("output")
                .short('o')
                .long("output")
                .value_name("FILE")
                .help("Output file (.wasm)"),
        )
        .arg(
            Arg::new("debuginfo")
                .short('g')
                .long("debuginfo")
                .action(ArgAction::SetTrue)
                .help("Preserve debug info"),
        )
        .arg(
            Arg::new("debug")
                .short('d')
                .long("debug")
                .action(ArgAction::SetTrue)
                .help("Print debug information"),
        )
        .arg(
            Arg::new("validate")
                .long("validate")
                .action(ArgAction::SetTrue)
                .help("Validate the module"),
        )
        .arg(
            Arg::new("no-validate")
                .long("no-validate")
                .action(ArgAction::SetTrue)
                .help("Do not validate the module"),
        )
        .arg(
            Arg::new("validate-globally")
                .long("validate-globally")
                .action(ArgAction::SetTrue)
                .help("Validate the module after each pass"),
        )
        .arg(
            Arg::new("opt-level")
                .short('O')
                .action(ArgAction::Append)
                .num_args(0..=1)
                .require_equals(true)
                .default_missing_value("2")
                .help("Optimization level (0, 1, 2, 3, 4, s, z)"),
        )
        .arg(
            Arg::new("pass-arg")
                .long("pass-arg")
                .action(ArgAction::Append)
                .value_name("KEY=VALUE")
                .help("Pass arguments (e.g. simplify-locals@allow-tee=true)"),
        )
        .arg(
            Arg::new("fast-math")
                .long("fast-math")
                .action(ArgAction::SetTrue)
                .help("Enable fast math optimizations"),
        )
        .arg(
            Arg::new("closed-world")
                .long("closed-world")
                .action(ArgAction::SetTrue)
                .help("Assume closed world (no external calls/imports can see internals)"),
        )
        .arg(
            Arg::new("traps-never-happen")
                .long("traps-never-happen")
                .action(ArgAction::SetTrue)
                .help("Assume traps never happen at runtime"),
        )
        .arg(
            Arg::new("low-memory-unused")
                .long("low-memory-unused")
                .action(ArgAction::SetTrue)
                .help("Assume low memory is unused"),
        )
        .arg(
            Arg::new("zero-filled-memory")
                .long("zero-filled-memory")
                .action(ArgAction::SetTrue)
                .help("Assume memory is zero-filled"),
        )
        .arg(
            Arg::new("emit-text")
                .short('S')
                .long("emit-text")
                .action(ArgAction::SetTrue)
                .help("Output text format (.wat)"),
        )
        .arg(
            Arg::new("print-wat-arg")
                .short('p')
                .long("print-wat")
                .action(ArgAction::SetTrue)
                .help("Print the module to stdout"),
        )
        .arg(
            Arg::new("print-rust")
                .long("print-rust")
                .action(ArgAction::SetTrue)
                .help("Print decompiled Rust code"),
        )
        .arg(
            Arg::new("print-c")
                .long("print-c")
                .action(ArgAction::SetTrue)
                .help("Print decompiled C code"),
        );

    // Register all feature flags
    let (mut cmd, feature_flag_ids) = add_feature_flags(cmd);

    // Register all passes as long flags using registry descriptions
    for info in PASS_REGISTRY {
        cmd = cmd.arg(
            Arg::new(info.name)
                .long(info.name)
                .action(ArgAction::Append)
                .num_args(0)
                .help(info.description),
        );
    }

    let matches = cmd.get_matches();

    let input_path: PathBuf = matches
        .get_one::<String>("input")
        .map(PathBuf::from)
        .unwrap();
    let output_path: Option<PathBuf> = matches.get_one::<String>("output").map(PathBuf::from);
    let debug_mode = matches.get_flag("debug");
    let validate = !matches.get_flag("no-validate");
    let validate_globally = matches.get_flag("validate-globally");

    let mut runner = PassRunner::new();
    runner.set_validate_globally(validate_globally);

    // Collect pass arguments
    if let Some(args) = matches.get_many::<String>("pass-arg") {
        for arg in args {
            if let Some((key, val)) = arg.split_once('=') {
                runner.pass_args.insert(key.to_string(), val.to_string());
            }
        }
    }

    // Collect all pass-related argument indices to preserve order
    let mut actions = Vec::new();

    // Collect Optimization Levels
    if let Some(indices) = matches.indices_of("opt-level") {
        let values: Vec<_> = matches
            .get_many::<String>("opt-level")
            .map(|v| v.collect())
            .unwrap_or_else(Vec::new);

        let mut val_iter = values.into_iter();
        for idx in indices {
            let level = val_iter.next().cloned().unwrap_or_else(|| "2".to_string());
            actions.push((idx, Action::Opt(level)));
        }
    }

    // Collect specific passes
    for name in PassRunner::get_all_pass_names() {
        if let Some(indices) = matches.indices_of(name) {
            for idx in indices {
                actions.push((idx, Action::Pass(name.to_string())));
            }
        }
    }

    // Sort by index to preserve order
    actions.sort_by_key(|(idx, _)| *idx);

    for (_, action) in actions {
        match action {
            Action::Opt(level) => {
                let mut opt = match level.as_str() {
                    "0" => OptimizationOptions::o0(),
                    "1" => OptimizationOptions::o1(),
                    "2" => OptimizationOptions::o2(),
                    "3" => OptimizationOptions::o3(),
                    "4" => OptimizationOptions::o4(),
                    "s" => OptimizationOptions::os(),
                    "z" => OptimizationOptions::oz(),
                    _ => anyhow::bail!("Unknown optimization level: {}", level),
                };

                // Apply global flags to this preset
                opt.fast_math = matches.get_flag("fast-math");
                opt.closed_world = matches.get_flag("closed-world");
                opt.traps_never_happen = matches.get_flag("traps-never-happen");
                opt.low_memory_unused = matches.get_flag("low-memory-unused");
                opt.zero_filled_memory = matches.get_flag("zero-filled-memory");
                opt.debug_info = matches.get_flag("debuginfo");

                runner.add_default_optimization_passes(&opt);
            }
            Action::Pass(name) => {
                if !runner.add_by_name(&name) {
                    anyhow::bail!("Unknown pass: --{}", name);
                }
            }
        }
    }

    let data = read_input(&input_path)?;

    let mut module = if data.starts_with(b"\0asm") {
        let mut reader = BinaryReader::new();
        reader
            .parse_module(&data)
            .map_err(|e| anyhow!("Binary parse error: {:?}", e))?
    } else {
        let text = std::str::from_utf8(&data).context("WAT input is not valid UTF-8")?;
        Module::read_wat(text).map_err(|e| anyhow!("WAT parse error: {}", e))?
    };

    // Apply features from flags
    apply_feature_flags(&mut module.features, &matches, &feature_flag_ids);

    if debug_mode {
        println!("Running passes...");
    }

    runner.run(&mut module);

    if validate {
        let (valid, errors) = Validator::new(&module).validate();
        if !valid {
            anyhow::bail!("Validation failed:\n{}", errors.join("\n"));
        }
    }

    if matches.get_flag("print-wat-arg") {
        let wat = module
            .to_wat()
            .map_err(|e| anyhow!("WAT generation failed: {}", e))?;
        println!("{}", wat);
    }

    if matches.get_flag("print-rust") {
        use washm::decompile::lifter::Lifter;
        use washm::decompile::printers::rust_printer::RustPrinter;

        let mut lifter = Lifter::new();
        lifter.run(&mut module);

        let mut printer = RustPrinter::new(&module);
        println!("{}", printer.print());
    }

    if matches.get_flag("print-c") {
        use washm::decompile::lifter::Lifter;
        use washm::decompile::printers::c_printer::CPrinter;

        let mut lifter = Lifter::new();
        lifter.run(&mut module);

        let mut printer = CPrinter::new(&module);
        println!("{}", printer.print());
    }

    if let Some(path) = output_path {
        if matches.get_flag("emit-text") {
            let wat = module
                .to_wat()
                .map_err(|e| anyhow!("WAT generation failed: {}", e))?;
            write_output(&path, wat.as_bytes())?;
        } else {
            let mut writer = BinaryWriter::new();
            let bytes = writer
                .write_module(&module)
                .map_err(|e| anyhow!("Write error: {:?}", e))?;
            write_output(&path, &bytes)?;
        }
    }

    Ok(())
}

enum Action {
    Opt(String),
    Pass(String),
}
