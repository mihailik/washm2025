// Decompiled to Rust from WebAssembly

fn add(acc: i32, acc1: i32) -> i32 {
    let mut v2: i32 = 0;
    return (acc + acc1);
}

fn factorial(v0: i32) -> i32 {
    let mut v1: i32 = 0;
    let mut v2: i32 = 0;
    let mut v3: i32 = 0;
    {
        v1 = 1;
        if unary((v0 < 1)) {
            v3 = 7;
            v1 = 1;
            v2 = 1;
            'shape_continue_4: loop {
                {
                    v1 = (v2 * v1);
                    ((v2 = (v2 + 1)) <= v0);
                }
            }
        }
        v1;
    }
}