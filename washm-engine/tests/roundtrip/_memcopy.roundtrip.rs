// Decompiled to Rust from WebAssembly
// MARKER_CHECK

fn memcopy(v0: i32, v1: i32, v2: i32) {
    let mut v3: i32 = 0;
    'block_break_1: {
        if !(v2) {
            break 'block_break_1;
        }
        v3 = 4;
        memory_copy(v1, v0, v2);
        if (v3 == 4) {
            memory_copy(v1, v0, v2);
        }
    }
    {
    }
}