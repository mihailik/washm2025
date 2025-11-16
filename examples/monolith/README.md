This example demonstrates building the root `washm` crate as a wasm32-wasi binary and running it under Wasmtime.

Steps:

1. Ensure the wasm target is installed:

   ```sh
   rustup target add wasm32-wasi
   ```

2. Build the wasm binary:

   ```sh
   cargo build --target wasm32-wasi
   ```

3. Run with Wasmtime (install it from https://wasmtime.dev):

   ```sh
   wasmtime target/wasm32-wasi/debug/washm.wasm -- echo hello world
   ```

Note: wasm builds are currently allowed to fail as we iterate on WASI support.
