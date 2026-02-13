// Decompiled to Rust from WebAssembly

fn add(v0: i32, v1: i32) -> i32 {
    let mut v2: i32 = 0;
    return (v0 + v1);
}

fn factorial(v0: i32) -> i32 {
    let mut v1: i32 = 0;
    let mut v2: i32 = 0;
    let mut v3: i32 = 0;
    if ((v4 = v3) == (v5 = 6)) {
        v1 = v6;
        v2 = v13;
        v1 = ((v7 = (v17 = v12)) * (v8 = v14));
        ((v2 = (v9 = ((v12 = v17) + (v13 = (v18 = v13))))) <= (v10 = v15));
        break;
        v1 = v6;
        if (v11 = (v15 < v13)) {
            break;
        } else {
            v3 = 6;
        }
        /* loop: shape$continue$3 */ loop {
            {
                v1 = ((v7 = (v17 = v12)) * (v8 = v14));
                ((v2 = (v9 = ((v12 = v17) + (v13 = (v18 = v13))))) <= (v10 = v15));
                v1 = ((v7 = (v17 = v12)) * (v8 = v14));
                ((v2 = (v9 = ((v12 = v17) + (v13 = (v18 = v13))))) <= (v10 = v15));
                break;
                if (v16 = (v15 < v13)) {
                    break;
                } else {
                    break;
                }
                v1 = v6;
                v2 = v13;
                v1 = ((v7 = (v17 = v12)) * (v8 = v14));
                ((v2 = (v9 = ((v12 = v17) + (v13 = (v18 = v13))))) <= (v10 = v15));
            }
        }
        loop {
            /* unsupported expr: LocalGet { id: Id(8:0) } */;
        }
    }
}