Persisting test reports
=======================

The `tinywasm` integration tests generate an in-memory report during test runs.
By default the tests do not write files to disk. To persist a report for investigation,
set the environment variable `TEST_SAVE_CSV_MODE` to one of:

- `off` (default): do not persist any files
- `per-test`: write a single JSON file per test target under `target/test-generated/`. The file name is `{crate}-{exe}.json` with the test binary name sanitized (cargo hash removed).
- `per-group`: write one JSON per test group under `target/test-generated/`.

Example (Windows cmd):

```
set TEST_SAVE_CSV_MODE=per-test && cd tinywasm-ssw && cargo test -p tinywasm --test test-wasm-1
```

When you use `per-test`, the test harness will write:

  `crates/tinywasm/target/test-generated/tinywasm-test_wasm_1.json`

The persisted format is JSON, not CSV, and contains a structured object:

- `version` - crate version
- `passed` - total passed
- `failed` - total failed
- `groups` - array of group stats (name/failed/passed)

If you want merged reports across multiple test executables, use the aggregator script in the workspace `scripts/` directory.
