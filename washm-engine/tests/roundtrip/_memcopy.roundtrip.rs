// Decompiled to Rust from WebAssembly

fn memcopy(v0: i32, v1: i32, v2: i32) {
    let mut v3: i32 = 0;
    if (v2 != 0) {
        v3 = 4;
        memory_copy(v1, v0, v2);
    }
}