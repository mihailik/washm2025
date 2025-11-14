mod testsuite;
use eyre::Result;
use testsuite::TestSuite;
use wasm_testsuite::data::{SpecVersion, spec};

fn main() -> Result<()> {
    TestSuite::set_log_level(log::LevelFilter::Off);

    let mut test_suite = TestSuite::new();
    test_suite.run_files(spec(&SpecVersion::V1))?;
    let _csv = test_suite.generate_csv(env!("CARGO_PKG_VERSION"))?;
    // Optionally persist per-test JSON outputs (preferred) - enable with `TEST_SAVE_CSV_MODE=per-test`.
    // Use `TEST_SAVE_CSV_MODE=per-group` to write a small JSON file per group.
    // Files are written to: `target/test-generated/{crate}-{exe}.json` (exe name sanitized to remove cargo hash).
    let _persist = test_suite.persist_csv_if_enabled(None, env!("CARGO_PKG_VERSION"))?;
    test_suite.report_status()
}
