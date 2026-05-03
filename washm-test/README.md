# washm-test

**washm-test** is a high-performance, self-instrumenting test runner for WebAssembly. It allows you to run standard Rust tests on the `wasm32-wasip1` target while automatically collecting code coverage without requiring any modifications to your source code.

## The "Holy Grail" of WASM Testing

The vision of `washm-test` is to provide a seamless bridge between standard `cargo test` workflows and WASM runtimes. By leveraging DWARF debug information, it distills your WASM binaries, injects lightweight coverage probes, and executes them in an embedded `wasmtime` environment.

### Key Features

- **Zero-Code Change Coverage**: No need for `#[cfg(test)]` instrumentation or custom macros.
- **Cargo Integration**: Plugs directly into `cargo test` via the `runner` configuration.
- **LCOV Export**: Generates standard `lcov.info` files compatible with VS Code extensions like *Coverage Gutters*.
- **Embedded Wasmtime**: Runs tests in a controlled, high-performance WASM environment.

## Getting Started

### 1. Install the toolchain
```bash
cargo install washm-test
rustup target add wasm32-wasip1
```

### 2. Configure your project
Add a `.cargo/config.toml` to your project root:
```toml
[target.wasm32-wasip1]
runner = "washm-run"
```

### 3. Run tests
```bash
cargo test --target wasm32-wasip1
```

## How It Works

1. **Distillation**: `washm-distill` parses the DWARF info in your compiled WASM binary.
2. **Instrumentation**: It patches the binary to include a "DMZ" (Data Management Zone) in memory and injects probes at every executable line.
3. **Execution**: `washm-run` executes the distilled binary using `wasmtime`.
4. **Collection**: On completion, it reads the DMZ memory to extract hit counts and maps them back to your source files, producing an LCOV report.

## License

This project is licensed under the MIT License - see the [LICENSE](../LICENSE) file for details.
