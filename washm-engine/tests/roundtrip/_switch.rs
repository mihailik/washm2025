extern "C" {
    fn side_effect(x: i32);
}

#[no_mangle]
pub extern "C" fn sw(x: i32) -> i32 {
    match x {
        0 => { unsafe { side_effect(0); } 10 },
        1 => { unsafe { side_effect(1); } 20 },
        2 => { unsafe { side_effect(2); } 30 },
        _ => { unsafe { side_effect(3); } 999 },
    }
}
