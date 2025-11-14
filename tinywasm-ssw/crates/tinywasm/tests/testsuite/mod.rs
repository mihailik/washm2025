#![allow(unused)]
use eyre::{Result, eyre};
use indexmap::IndexMap;
use owo_colors::OwoColorize;
use std::io::{BufRead, Seek, SeekFrom};
use std::{
    collections::BTreeMap,
    fmt::{Debug, Formatter},
    io::BufReader,
};

mod run;
pub mod util;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct TestGroupResult {
    pub name: String,
    pub passed: usize,
    pub failed: usize,
}

pub struct TestSuite(BTreeMap<String, TestGroup>, Vec<String>);

impl TestSuite {
    pub fn set_log_level(level: log::LevelFilter) {
        pretty_env_logger::formatted_builder().filter_level(level).init();
    }

    pub fn report_status(&self) -> Result<()> {
        if self.failed() {
            println!();
            Err(eyre!(format!("{}:\n{:#?}", "failed one or more tests".red().bold(), self)))
        } else {
            println!("\n\npassed all tests:\n{self:#?}");
            Ok(())
        }
    }

    pub fn print_errors(&self) {
        for (group_name, group) in &self.0 {
            let tests = &group.tests;
            for (test_name, test) in tests {
                if let Err(e) = &test.result {
                    eprintln!(
                        "{} {} failed: {:?}",
                        link(group_name, &group.file, Some(test.linecol.0 + 1)).bold().underline(),
                        test_name.bold(),
                        e.to_string().bright_red()
                    );
                }
            }
        }
    }

    pub fn new() -> Self {
        Self(BTreeMap::new(), Vec::new())
    }

    pub fn skip(&mut self, name: &str) {
        self.1.push(name.to_string());
    }

    pub fn failed(&self) -> bool {
        self.0.values().any(|group| group.stats().1 > 0)
    }

    fn test_group(&mut self, name: &str, file: &str) -> &mut TestGroup {
        self.0.entry(name.to_string()).or_insert_with(|| TestGroup::new(file))
    }

    // create or add to a test result file
    pub fn save_csv(&self, path: &str, version: &str) -> Result<()> {
        use std::fs::OpenOptions;
        use std::io::Write;

        // If we can't open the CSV file (e.g. permission denied, file locked by IDE),
        // do not fail the test suite — the CSV is only used for local test reporting.
        use std::path::{Path, PathBuf};

        // Avoid writing into the repository path (e.g. ./tests/generated/...),
        // which can be locked by tools/IDE and cause permission denied errors on Windows.
        // Instead, write CSV output under `<manifest_dir>/target/test-generated/`.
        let mut output_path: PathBuf = Path::new(path).to_path_buf();
        if output_path.starts_with("tests") || output_path.starts_with("./tests") {
            let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
            let file_name = output_path.file_name().map(|s| s.to_owned()).unwrap_or_else(|| "generated.csv".into());
            let new_dir = Path::new(&manifest_dir).join("target").join("test-generated");
            std::fs::create_dir_all(&new_dir)?;
            output_path = new_dir.join(file_name);
        }

        // Open the new path for append and read.
        let mut file = OpenOptions::new().create(true).append(true).read(true).open(output_path.clone())?;
        let last_line = BufReader::new(&file).lines().last().transpose()?;

        // Check if the last line starts with the current commit
        if let Some(last) = last_line
            && last.starts_with(version)
        {
            // Truncate the file size to remove the last line
                let len_to_truncate = last.len() as i64;
                file.set_len(file.metadata()?.len() - len_to_truncate as u64 - 1)?;
        }

        // Seek to the end of the file for appending
        file.seek(SeekFrom::End(0))?;

        let mut passed = 0;
        let mut failed = 0;

        let mut groups = Vec::new();
        for (name, group) in &self.0 {
            let (group_passed, group_failed) = group.stats();
            passed += group_passed;
            failed += group_failed;

            groups.push(TestGroupResult { name: name.to_string(), passed: group_passed, failed: group_failed });
        }

        let groups = serde_json::to_string(&groups)?;
        let line = format!("{version},{passed},{failed},{groups}\n");
        file.write_all(line.as_bytes())?;

        Ok(())
    }

    /// Generate a CSV report string in-memory instead of writing to disk.
    ///
    /// Tests should call this method to avoid filesystem writes during testing.
    pub fn generate_csv(&self, version: &str) -> Result<String> {
        let mut passed = 0;
        let mut failed = 0;

        let mut groups = Vec::new();
        for (name, group) in &self.0 {
            let (group_passed, group_failed) = group.stats();
            passed += group_passed;
            failed += group_failed;

            groups.push(TestGroupResult { name: name.to_string(), passed: group_passed, failed: group_failed });
        }

        let groups = serde_json::to_string(&groups)?;
        let line = format!("{version},{passed},{failed},{groups}\n");
        Ok(line)
    }

    /// Generate a structured JSON report describing the test run.
    /// This is the preferred format for persisted test artifacts because JSON is
    /// unambiguous and avoids the unfortunate pattern of embedding JSON inside
    /// CSV text.
    pub fn generate_report(&self, version: &str) -> Result<String> {
        #[derive(Serialize)]
        struct Report<'a> {
            version: &'a str,
            passed: usize,
            failed: usize,
            groups: &'a [TestGroupResult],
        }

        let mut passed = 0;
        let mut failed = 0;
        let mut groups = Vec::new();
        for (name, group) in &self.0 {
            let (group_passed, group_failed) = group.stats();
            passed += group_passed;
            failed += group_failed;
            groups.push(TestGroupResult { name: name.to_string(), passed: group_passed, failed: group_failed });
        }

        let report = Report { version, passed, failed, groups: &groups };
        Ok(serde_json::to_string_pretty(&report)?)
    }

    /// Persist CSV output when enabled in the environment.
    ///
    /// Uses `TEST_SAVE_CSV_MODE` env var with values:
    /// - "off" (default): no file writes.
    /// - "per-test": write one file per test target / binary, with a deterministic name.
    /// - "per-group": write one CSV file per test group (useful for fine-grained analysis).
    pub fn persist_csv_if_enabled(&self, prefix: Option<&str>, version: &str) -> Result<Vec<std::path::PathBuf>> {
        let mode = std::env::var("TEST_SAVE_CSV_MODE").unwrap_or_else(|_| "off".to_string()).trim().to_lowercase();
        if mode == "off" || mode.is_empty() {
            return Ok(Vec::new());
        }

        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
        let out_dir = std::path::Path::new(&manifest_dir).join("target").join("test-generated");
        std::fs::create_dir_all(&out_dir)?;

        let prefix = match prefix {
            Some(p) => p.to_string(),
            None => {
                // Default prefix: {crate_name}-{exe}
                let crate_name = std::env::var("CARGO_PKG_NAME").unwrap_or_else(|_| "unknown_crate".to_string());
                let exe_name = std::env::args().next().and_then(|a| std::path::Path::new(&a).file_stem().and_then(|s| s.to_str().map(|s| s.to_string()))).unwrap_or_else(|| "unknown_test".to_string());
                // Remove the trailing cargo-generated hash from the exe name
                // (e.g. "test_wasm_1-940e60012ac454d4") so the persisted
                // artifact is human-friendly and stable.
                fn sanitize_exe_name(name: &str) -> String {
                    if let Some(pos) = name.rfind('-') {
                        let tail = &name[pos + 1..];
                        if tail.len() >= 8 && tail.chars().all(|c| c.is_ascii_hexdigit()) {
                            return name[..pos].to_string();
                        }
                    }
                    name.to_string()
                }

                format!("{}-{}", crate_name, sanitize_exe_name(&exe_name))
            }
        };

        // Sanitize the prefix to avoid unusual characters in filenames.
        fn sanitize_for_filename(s: &str) -> String {
            s.chars()
                .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
                .collect()
        }

        let prefix = sanitize_for_filename(&prefix);

        let mut persisted = Vec::new();
        // No debug output by default to avoid polluting test logs.

        if mode == "per-test" {
            // Persist CSV per test target in a deterministic name so it can be inspected
            // in between runs. This overwrites the previous run's file (no per-run history).
            // Use JSON output instead of embedding JSON inside CSV fields.
            let report_json = self.generate_report(version)?;
            let fname = format!("{prefix}.json");
            let final_path = out_dir.join(fname);
            // Write via temp+rename for atomicity
            // Write to a temporary file and atomically rename to final path to avoid
            // partial files being visible to outside tools.
            let tmp_name = format!("{}.tmp", final_path.display());
            let tmp_path = std::path::Path::new(&tmp_name);
            std::fs::write(tmp_path, report_json.as_bytes())?;
            std::fs::rename(tmp_path, &final_path)?;
            persisted.push(final_path);
            return Ok(persisted);
        }

        if mode == "per-group" {
            // Create one file per group with a simple CSV of group stats
            for (name, group) in &self.0 {
                let group_fname = format!("{prefix}.{}.json", sanitize_for_filename(name));
                let final_path = out_dir.join(group_fname);
                let mut buf = String::new();
                buf.push_str("version,group,passed,failed\n");
                let (passed, failed) = group.stats();
                // Store the group summary as a small JSON object for clarity.
                let mut j = serde_json::Map::new();
                j.insert("version".to_string(), serde_json::Value::String(version.to_string()));
                j.insert("group".to_string(), serde_json::Value::String(name.to_string()));
                j.insert("passed".to_string(), serde_json::Value::Number(serde_json::Number::from(passed)));
                j.insert("failed".to_string(), serde_json::Value::Number(serde_json::Number::from(failed)));
                let buf = serde_json::Value::Object(j).to_string();
                // atomic write
                let tmp_name = format!("{}.tmp", final_path.display());
                let tmp_path = std::path::Path::new(&tmp_name);
                std::fs::write(tmp_path, buf.as_bytes())?;
                std::fs::rename(tmp_path, &final_path)?;
                persisted.push(final_path);
            }
            return Ok(persisted);
        }

        // Unknown mode: return error
        Err(eyre!(format!("unknown TEST_SAVE_CSV_MODE: {}", mode)))
    }
}

fn link(name: &str, file: &str, line: Option<usize>) -> String {
    let (path, name) = match line {
        None => (file.to_string(), name.to_owned()),
        Some(line) => (format!("{file}:{line}:0"), (format!("{name}:{line}"))),
    };

    format!("\x1b]8;;file://{path}\x1b\\{name}\x1b]8;;\x1b\\")
}

impl Debug for TestSuite {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut total_passed = 0;
        let mut total_failed = 0;

        for (group_name, group) in &self.0 {
            let (group_passed, group_failed) = group.stats();
            total_passed += group_passed;
            total_failed += group_failed;

            writeln!(f, "{}", link(group_name, &group.file, None).bold().underline())?;
            writeln!(f, "  Tests Passed: {}", group_passed.to_string().green())?;

            if group_failed != 0 {
                writeln!(f, "  Tests Failed: {}", group_failed.to_string().red())?;
            }
        }

        writeln!(f, "\n{}", "Total Test Summary:".bold().underline())?;
        writeln!(f, "  Total Tests: {}", (total_passed + total_failed))?;
        writeln!(f, "  Total Passed: {}", total_passed.to_string().green())?;
        writeln!(f, "  Total Failed: {}", total_failed.to_string().red())?;
        Ok(())
    }
}

struct TestGroup {
    tests: IndexMap<String, TestCase>,
    file: String,
}

impl TestGroup {
    fn new(file: &str) -> Self {
        Self { tests: IndexMap::new(), file: file.to_string() }
    }

    fn stats(&self) -> (usize, usize) {
        let mut passed_count = 0;
        let mut failed_count = 0;

        for test in self.tests.values() {
            match test.result {
                Ok(()) => passed_count += 1,
                Err(_) => failed_count += 1,
            }
        }

        (passed_count, failed_count)
    }

    fn add_result(&mut self, name: &str, linecol: (usize, usize), result: Result<()>) {
        self.tests.insert(name.to_string(), TestCase { result, linecol });
    }
}

#[derive(Debug)]
struct TestCase {
    result: Result<()>,
    linecol: (usize, usize),
}
