mod testsuite;
use eyre::Result;
use testsuite::TestSuite;
use wasm_testsuite::data::{SpecVersion, spec};

fn main() -> Result<()> {
    TestSuite::set_log_level(log::LevelFilter::Off);

    let mut test_suite = TestSuite::new();
    test_suite.run_files(spec(&SpecVersion::V2))?;
    let _csv = test_suite.generate_csv(env!("CARGO_PKG_VERSION"))?;
    let _persist = test_suite.persist_csv_if_enabled(None, env!("CARGO_PKG_VERSION"))?;
    test_suite.report_status()
}
