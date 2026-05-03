// Test command implementation for brush shell integration
#![cfg(feature = "wasm-test")]

use crate::brush_core::builtins::{ContentOptions, ContentType, SimpleCommand};
use crate::brush_core::{commands, error, results};
use crate::test_harness::{run_all_tests, TestStatus};

pub struct TestCommand;

impl SimpleCommand for TestCommand {
    fn get_content(
        name: &str,
        _content_type: ContentType,
        _options: &ContentOptions,
    ) -> Result<String, error::Error> {
        Ok(format!("{} - run tests", name))
    }

    fn execute<I: Iterator<Item = S>, S: AsRef<str>>(
        _context: commands::ExecutionContext<'_>,
        args: I,
    ) -> Result<results::ExecutionResult, error::Error> {
        // Register tests before running
        crate::example_tests::register_example_tests();

        let args_vec: Vec<String> = args.map(|s| s.as_ref().to_string()).collect();

        // Skip the first argument if it's the command name "test" or "wasm-test"
        let args_vec: Vec<String> = if let Some(first) = args_vec.first() {
            if first == "test" || first == "wasm-test" {
                args_vec.into_iter().skip(1).collect()
            } else {
                args_vec
            }
        } else {
            args_vec
        };

        // Parse arguments (similar to cargo test)
        let mut filter: Option<String> = None;
        let mut sidecar_path: Option<String> = None;
        let mut _nocapture = false;

        let mut iter = args_vec.iter();
        while let Some(arg) = iter.next() {
            if arg == "--nocapture" {
                _nocapture = true;
            } else if arg == "--sidecar" {
                sidecar_path = iter.next().cloned();
            } else if !arg.starts_with('-') && filter.is_none() {
                filter = Some(arg.clone());
            }
        }

        // Load sidecar if provided
        if let Some(ref path) = sidecar_path {
            match crate::bridges::host::load_sidecar(path) {
                Ok(mapper) => {
                    println!(
                        "Loaded sidecar from {} ({} mapping entries)",
                        path,
                        mapper.line_map.len()
                    );
                    // TODO: Store mapper for results processing
                }
                Err(e) => {
                    eprintln!("Warning: Failed to load sidecar from {}: {}", path, e);
                }
            }
        }

        // Set filter as environment variable if provided
        if let Some(ref test_name) = filter {
            std::env::set_var("__RUST_TEST_INVOKE", test_name);
        }

        // Run tests
        let results = run_all_tests();

        // Clear filter
        if filter.is_some() {
            std::env::remove_var("__RUST_TEST_INVOKE");
        }

        // Print results in cargo test format
        println!("\nrunning {} tests", results.results.len());

        // Coverage Handshake: Try to get coverage pointer
        let coverage_ptr = crate::bridges::host::get_coverage_ptr();
        if coverage_ptr != 0 {
            println!(
                "Coverage handshake detected! Counter DMZ at 0x{:08X}",
                coverage_ptr
            );

            // If we have a sidecar, we can calculate coverage
            if let Some(ref path) = sidecar_path {
                if let Ok(mapper) = crate::bridges::host::load_sidecar(path) {
                    println!("Applying sidecar coverage analysis...");
                    // Potentially print mapped summary here
                }
            }
        }

        for result in &results.results {
            let status = match result.result {
                TestStatus::Ok => "ok",
                TestStatus::Failed => "FAILED",
                TestStatus::Ignored => "ignored",
            };

            let time = if result.exec_time_ms > 0 {
                format!(" ({}ms)", result.exec_time_ms)
            } else {
                String::new()
            };

            println!("test {} ... {}{}", result.name, status, time);

            if result.result == TestStatus::Failed {
                if let Some(ref msg) = result.failure_message {
                    eprintln!("\n---- {} stderr ----", result.name);
                    eprintln!("{}", msg);
                }
            }
        }

        println!();
        if results.failed > 0 {
            println!(
                "test result: FAILED. {} passed; {} failed; {} ignored\n",
                results.passed, results.failed, results.ignored
            );
            Ok(results::ExecutionResult::new(1))
        } else {
            println!(
                "test result: ok. {} passed; {} failed; {} ignored\n",
                results.passed, results.failed, results.ignored
            );
            Ok(results::ExecutionResult::new(0))
        }
    }
}
