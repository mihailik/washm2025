Node Host for WASI-Async PoC

See `host/node-host/host.js`.

Build & run
1. Build the WASM guest (the root crate is multi-target):

```bash
rustup target add wasm32-wasip1
cargo build --target wasm32-wasip1
```

2. Run Node host (Point at the WASM produced):

```bash
node host/node-host/host.js target/wasm32-wasip1/debug/washm.wasm
```

This will instantiate the WASM guest, watch `host_eval` for `*.js` requests, execute the JS payload in a `vm` context, write `{id}.json` results, and call guest `_continue`.
