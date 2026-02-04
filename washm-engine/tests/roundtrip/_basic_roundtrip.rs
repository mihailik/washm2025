// Decompiled to Rust from WebAssembly

fn func_0(v0: i32, v1: i32) -> i32 {
    let mut v2: i32 = 0;
    return /* unsupported expr: Binary { op: AddInt32, left: Id(60:0), right: Id(61:0) } */;
}

fn func_1(v0: i32) -> i32 {
    let mut v1: i32 = 0;
    let mut v2: i32 = 0;
    let mut v3: i32 = 0;
    {
        v1 = (v4 = 1);
        v1 = v4;
        v2 = v4;
    }
}

fn func_2() {
    let mut v0: i32 = 0;
    /* nop */
}

fn func_3() {
    let mut v0: i32 = 0;
    {
        func_2();
        func_2();
    }
}

fn func_4(v0: i32, v1: i32) -> i32 {
    let mut v2: i32 = 0;
    {
        func_0((v3 = v0), (v4 = v1));
        func_3()
    }
}

fn func_5(v0: i32) -> i32 {
    let mut v1: i32 = 0;
    {
        func_1((v2 = v0));
        func_3()
    }
}

