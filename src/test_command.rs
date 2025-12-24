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

        // Skip the first argument if it's the command name "test"
        let args_vec: Vec<String> = if args_vec.first().map(|s| s.as_str()) == Some("test") {
            args_vec.into_iter().skip(1).collect()
        } else {
            args_vec
        };

        // Parse arguments (similar to cargo test)
        let mut filter: Option<String> = None;
        let mut _nocapture = false;

        for arg in args_vec.iter() {
            if arg == "--nocapture" {
                _nocapture = true;
            } else if !arg.starts_with('-') && filter.is_none() {
                filter = Some(arg.clone());
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
