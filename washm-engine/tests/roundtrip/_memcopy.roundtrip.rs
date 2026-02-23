// Decompiled to Rust from WebAssembly

fn memcopy(v0: i32, v1: i32, v2: i32) {
    let mut v3: i32 = 0;
    {
        'block_break_18: {
            if !(v2) {
                break 'block_break_1;
            }
            memory_copy(v1, v0, v2);
        }
        {
        }
    }
}