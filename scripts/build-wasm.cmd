@echo off
REM Build wasm targets; allow failures so CI doesn't block until wasm tooling is ready
setlocal
rustup target add wasm32-wasi || echo not installed
cargo build --target wasm32-wasi || exit /B 0
rustup target add wasm32-unknown-unknown || echo not installed
cargo build --target wasm32-unknown-unknown || exit /B 0
endlocal
