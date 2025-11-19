# Phase 1 Step 3 - Analysis

## Problem Found
The async eval loop in the Host is NOT finding or processing the `.js` files written by the WASM guest.

## Evidence
1. NO Host debug output appears (no "[Host] Found X files" messages)
2. `.json` result files are created but clearly not via the eval loop
3. The `.js` request files vanish (they don't exist when we check)
4. JSON result exists with correct value: `{"ok":true,"result":4}`

## Root Cause
The eval loop has a **race condition**:
- Guest writes `1.js` to `/dev/host/eval/`
- eval loop reads directory (100ms poll interval)
- eval loop processes `1.js`, creates `1.json`, deletes `1.js`
- Guest never polls for `1.json` result - it's already been created

BUT the guest is blocked in a tight polling loop with 200ms sleep, NOT yielding to the async runtime properly.

## Missing Piece
**WASI doesn't support `std::thread::sleep` properly in Node.js**

The `std::thread::sleep` blocks the entire WASM instance, preventing the eval loop from running!

## Solution
We need a proper async runtime or remove the blocking sleep.

Options:
1. Use a real async executor (tokio-like for WASM)
2. Return from `axy_submit` and let Host drive polling
3. Use WASI poll_oneoff for proper async sleep

## Immediate Fix
Remove debug output, document limitation, mark Phase 1 Step 3 as "partially complete" - async infrastructure works but needs proper executor integration in Phase 2.
