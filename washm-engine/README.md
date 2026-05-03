# washm

Core engine for the WASHM WebAssembly toolchain. This is a Rust port of Binaryen's core functionality, providing:

- WebAssembly Intermediate Representation (IR)
- WASM Binary and Text (WAT) parsing and emission
- Optimization and transformation passes
- Analysis utilities (call graphs, etc.)

## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
washm = "0.5.0"
```

## Features

- **Binaryen Compatibility**: Aims for high compatibility with Binaryen's IR and optimization passes.
- **Pure Rust**: Written in pure Rust for safety and performance (no C++ dependency).
- **Consolidated API**: All core logic integrated into a single crate.

## License

MIT
