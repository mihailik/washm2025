@echo off
REM Build wasm32-wasi and run with wasmtime. Allows wasm step to fail safely.
setlocal
rustup target add wasm32-wasi || echo target already present
cargo build --target wasm32-wasi || exit /B 1
if not exist target\wasm32-wasi\debug\washm.wasm (
  echo Build failed: wasm not found
  exit /B 1
)
wasmtime target\wasm32-wasi\debug\washm.wasm -- echo hello wasm
endlocal
