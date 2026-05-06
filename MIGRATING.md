# Migrating from `reedline` 0.47 to `reedline-async` 0.1.0

This document outlines the differences between the upstream `reedline` crate (up to version 0.47) and the async-native `reedline-async` fork (version 0.1.0). 

## 1. Async Trait Signatures
Key traits now use `Pin<Box<dyn Future>>` instead of returning synchronous values. If you implement these traits, you must wrap your return values and adapt to async execution logic. Affected traits are:
- `Completer`
- `Hinter`
- `Highlighter`
- `Validator`
- `History`
- `Menu`

## 2. Removed `read_line`
The synchronous `read_line` method has been replaced with an asynchronous approach. All consumers will need to `await` it inside an async context or use an executor such as `pollster::block_on` (or `tokio`) within their `main` loop.

## 3. Removed `ExternalPrinter`
The `ExternalPrinter` implementation has been removed. Downstream users relying on `ExternalPrinter` should look forward to an async `Reedline::print_above_prompt` API in future releases.

## 4. Feature Flag `crossterm-backend`
To support building on Wasm targets (`wasm32-*`), the library now gates `crossterm` and `fd-lock` behind the `crossterm-backend` feature flag, which is enabled by default for native targets. Ensure you disable default features if you are configuring a project pointing at WebAssembly.