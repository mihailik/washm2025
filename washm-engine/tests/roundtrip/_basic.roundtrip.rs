// Decompiled to Rust from WebAssembly
// MARKER_CHECK

fn add(acc: i32, acc1: i32) -> i32 {
    let mut v2: i32 = 0;
    return (acc + acc1);
}

fn factorial(v0: i32) -> i32 {
    let mut v1: i32 = 0;
    let mut v2: i32 = 0;
    let mut v3: i32 = 0;
    'block_break_2: {
        v1 = 1;
        if (v0 < 1) {
            break 'block_break_2;
        }
        v1 = 1;
        v2 = 1;
        loop {
            v1 = (v2 * v1);
            if !(({ v2 = (v2 + 1); v2 } <= v0)) {
                break 'block_break_9;
            }
        }
    }
    {
        return v1;
    }
}