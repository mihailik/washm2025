// Decompiled to Rust from WebAssembly
// MARKER_CHECK

fn side_effect(p0: i32); // import from env.side_effect

fn sw(v0: i32) -> i32 {
    let mut v1: i32 = 0;
    let mut v2: i32 = 0;
    side_effect(if { v1 = (v0 < 3); v1 } { v0 } else { 3 });
    return if v1 { ((v0 * 10) + 10) } else { 999 };
}