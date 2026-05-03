// Test harness for running tests in WASM
// Implements the PoC design from docs/1.3-test.md

use serde::{Deserialize, Serialize};
use std::panic::{self, AssertUnwindSafe};
use std::time::Instant;

#[derive(Debug, Serialize, Deserialize)]
pub struct TestResult {
    pub name: String,
    pub result: TestStatus,
    pub exec_time_ms: u64,
    pub stdout: String,
    pub stderr: String,
    pub failure_message: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TestStatus {
    Ok,
    Failed,
    Ignored,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TestResults {
    pub results: Vec<TestResult>,
    pub passed: usize,
    pub failed: usize,
    pub ignored: usize,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct SourceMapper {
    /// Maps binary offset to (source_file, line_number)
    pub line_map: std::collections::BTreeMap<u32, (String, u64)>,
}

pub struct Test {
    pub name: &'static str,
    pub test_fn: fn(),
    pub is_async: bool,
}

pub struct AsyncTest {
    pub name: &'static str,
    pub test_fn: fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = ()>>>,
}

use std::cell::RefCell;

// Global test registry
thread_local! {
    static TESTS: RefCell<Vec<Test>> = RefCell::new(Vec::new());
    static ASYNC_TESTS: RefCell<Vec<AsyncTest>> = RefCell::new(Vec::new());
}

pub fn register_test(name: &'static str, test_fn: fn()) {
    TESTS.with(|tests| {
        tests.borrow_mut().push(Test {
            name,
            test_fn,
            is_async: false,
        });
    })
}

pub fn register_async_test(
    name: &'static str,
    test_fn: fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = ()>>>,
) {
    ASYNC_TESTS.with(|tests| {
        tests.borrow_mut().push(AsyncTest { name, test_fn });
    })
}

fn run_single_test(test: &Test) -> TestResult {
    let start = Instant::now();
    let mut stdout = String::new();
    let stderr = String::new();

    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        (test.test_fn)();
    }));

    let exec_time_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(_) => TestResult {
            name: test.name.to_string(),
            result: TestStatus::Ok,
            exec_time_ms,
            stdout,
            stderr,
            failure_message: None,
        },
        Err(err) => {
            let failure_message = if let Some(s) = err.downcast_ref::<String>() {
                s.clone()
            } else if let Some(s) = err.downcast_ref::<&str>() {
                s.to_string()
            } else {
                "Test panicked".to_string()
            };

            TestResult {
                name: test.name.to_string(),
                result: TestStatus::Failed,
                exec_time_ms,
                stdout,
                stderr,
                failure_message: Some(failure_message),
            }
        }
    }
}

fn run_single_async_test(test: &AsyncTest) -> TestResult {
    let start = Instant::now();
    let stdout = String::new();
    let stderr = String::new();

    // Run async test using the current runtime handle
    let handle = tokio::runtime::Handle::current();
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let future = (test.test_fn)();
        handle.block_on(future)
    }));

    let exec_time_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(_) => TestResult {
            name: test.name.to_string(),
            result: TestStatus::Ok,
            exec_time_ms,
            stdout,
            stderr,
            failure_message: None,
        },
        Err(err) => {
            let failure_message = if let Some(s) = err.downcast_ref::<String>() {
                s.clone()
            } else if let Some(s) = err.downcast_ref::<&str>() {
                s.to_string()
            } else {
                "Test panicked".to_string()
            };

            TestResult {
                name: test.name.to_string(),
                result: TestStatus::Failed,
                exec_time_ms,
                stdout,
                stderr,
                failure_message: Some(failure_message),
            }
        }
    }
}

pub fn run_all_tests() -> TestResults {
    let mut results = Vec::new();
    let mut passed = 0;
    let mut failed = 0;
    let ignored = 0;

    // Check if we should run only a single test
    let single_test = std::env::var("__RUST_TEST_INVOKE").ok();

    // Run sync tests
    TESTS.with(|tests| {
        for test in tests.borrow().iter() {
            if let Some(ref filter) = single_test {
                if test.name != filter {
                    continue;
                }
            }

            let result = run_single_test(test);
            match result.result {
                TestStatus::Ok => passed += 1,
                TestStatus::Failed => failed += 1,
                TestStatus::Ignored => {}
            }
            results.push(result);
        }
    });

    // Run async tests
    ASYNC_TESTS.with(|tests| {
        for test in tests.borrow().iter() {
            if let Some(ref filter) = single_test {
                if test.name != filter {
                    continue;
                }
            }

            let result = run_single_async_test(test);
            match result.result {
                TestStatus::Ok => passed += 1,
                TestStatus::Failed => failed += 1,
                TestStatus::Ignored => {}
            }
            results.push(result);
            println!("Pushed result. Count: {}", results.len());
        }
    });

    println!("Finished running tests. Results count: {}", results.len());

    TestResults {
        results,
        passed,
        failed,
        ignored,
    }
}
