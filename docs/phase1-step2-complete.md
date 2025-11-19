# Phase 1, Step 2: uutils Integration - COMPLETE ✅

## Summary
Successfully integrated `uutils/coreutils` into the WASI Reactor pattern. The `washm` shell can now execute `ls` and other coreutils commands directly via in-process Rust function calls.

## Changes Made

### 1. Coreutils Fork - WASI Support
Created official fork at `coreutils-fork/` with WASI compatibility patches:

#### `uucore/src/lib/features/fs.rs`
- Added `#[cfg(target_os = "wasi")]` blocks for `FileInformation` struct
- Implemented WASI-specific stub methods:
  - `number_of_links()` → returns 1 (dummy)
  - `inode()` → returns 0 (dummy)
  - `PartialEq` → returns `false` (conservative)
  - `Hash` → hashes 0 (dummy)
- Added WASI support to `sane_blksize` module (returns DEFAULT)
- Added WASI stub for `is_stdin_directory()` (returns `false`)

#### `uucore/src/lib/features/fsext.rs`
- Added `target_os = "wasi"` to `read_fs_list()` empty stub list

#### `uucore/src/lib/mods/io.rs`
- Fixed `into_stdio()` to work with WASI by using `Stdio::from(self.into_file())`

### 2. Stub Dependencies
- Created `stubs/hostname/` to replace platform-specific `hostname` crate
- Patched `Cargo.toml` to use stub instead of crates.io version

### 3. Washm Integration
#### `src/lib.rs`
- Wired `axy_submit` to call `uu_ls::uumain()` directly
- Converts String args to `OsString` iterator (required by uucore)
- Returns exit code from uutils functions
- Added `echo` built-in command with explicit stdout flush

#### `Cargo.toml`
- Changed `crate-type = ["cdylib"]` for WASI Reactor
- Added dependencies:
  ```toml
  uu_ls = { path = "coreutils-fork/src/uu/ls" }
  uucore = { path = "coreutils-fork/src/uucore" }
  ```
- Patched hostname dependency to use local stub

### 4. Host Updates
#### `host/node-host/host.js`
- Added `args: ['washm']` to WASI constructor (fix empty env::args panic)
- Added preopen for current directory: `'.': process.cwd()`
- Allows `ls` to access filesystem

## Verification

Successfully tested:
```bash
$ echo -e "ls\necho hello world\nls -l Cargo.toml" | node host/node-host/host.js target/wasm32-wasip1/release/washm.wasm

axy> ls
Cargo.lock
Cargo.toml
LICENSE
...

axy> echo hello world
hello world

axy> ls -l Cargo.toml
-rwxrwxrwx 1 somebody somegroup 518 Nov 19 22:55 Cargo.toml
```

## Technical Notes

### WASI Limitations Addressed
1. **No std::os::unix**: WASI target doesn't have Unix-specific APIs
2. **Unstable wasi_ext**: Avoided using unstable `std::os::wasi::fs::MetadataExt`
3. **Metadata stubs**: File metadata (inode, device, links) stubbed with safe dummy values
4. **Stdout buffering**: Used explicit `write!` + `flush()` for echo to work around Node WASI buffering

### Fork Maintenance Strategy
- Maintain minimal diff from upstream using `#[cfg(target_os = "wasi")]`
- All patches are additive (no removals)
- Ready for upstreaming when uutils officially adopts WASI support

## Build Stats
- **Release binary size**: ~2.5MB optimized
- **Compile time**: ~18s for release build
- **Warnings**: 11 warnings (mostly unused imports and dead code - safe to ignore)

## Next Steps
Phase 1, Step 3: Implement Async Event Loop (`_continue` and callback mechanism)
