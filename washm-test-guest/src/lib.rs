//! The Infiltrator: Guest runtime for washm-test.

#[no_mangle]
pub static mut __WASHM_COV_PTR: *mut u8 = std::ptr::null_mut();

/// Macro to initialize coverage symbols.
#[macro_export]
macro_rules! init {
    () => {
        // This ensures the symbol is kept and exported
        #[no_mangle]
        pub static mut __WASHM_COV_PTR: *mut u8 = core::ptr::null_mut();
    };
}

pub fn do_math(a: i32, b: i32) -> i32 {
    if a > 10 {
        a + b * 2
    } else {
        a * b - 3
    }
}

pub fn untested_math(a: i32, b: i32) -> i32 {
    a + b + a + b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_do_math_high() {
        let a = std::hint::black_box(20);
        let b = std::hint::black_box(5);
        assert_eq!(do_math(a, b), 30);
    }

    #[test]
    fn test_do_math_low() {
        let a = std::hint::black_box(5);
        let b = std::hint::black_box(5);
        assert_eq!(do_math(a, b), 22);
    }
}
