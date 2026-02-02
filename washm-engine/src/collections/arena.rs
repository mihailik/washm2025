use bumpalo::Bump;
use std::os::raw::c_char;
use std::sync::Mutex;

/// Arena provides simple bump-allocated memory for short-lived allocations.
///
/// Ownership / Lifetime contract:
/// - Any pointer returned by `alloc_str` remains valid for as long as the
///   Arena is alive. Once `Arena` is dropped, all pointers previously
///   returned by `alloc_str` become invalid and must not be dereferenced.
/// - The arena does NOT allocate C-owned memory. C callers must not
///   free those pointers. Instead, callers should keep the pointer and use
///   it only while the Arena is alive.
/// - Allocated strings are null-terminated and safe to be passed to C code.
///
/// Thread-safety:
/// - This implementation uses a Mutex around the bump allocator, so it is
///   safe to call `alloc_str` concurrently from multiple threads.
/// - The Mutex provides correctness for concurrent allocations; however
///   the underlying bump allocator still frees the memory when the Arena is
///   dropped.
pub struct Arena {
    bump: Mutex<Bump>,
}

impl Default for Arena {
    fn default() -> Self {
        Self::new()
    }
}

impl Arena {
    /// Create a new Arena instance.
    pub fn new() -> Self {
        Arena {
            bump: Mutex::new(Bump::new()),
        }
    }

    /// Allocate a string in the arena and return a pointer valid for the arena lifetime.
    ///
    /// Returns a pointer to a null-terminated C string; the pointer remains
    /// valid until the Arena is dropped. The returned pointer must not be freed
    /// by the caller.
    pub fn alloc_str(&self, s: &str) -> *const c_char {
        // allocate a CString containing the data in the shared bump allocator
        let bytes = s.as_bytes();
        // create a vector with a trailing null byte
        let mut tmp: Vec<u8> = Vec::with_capacity(bytes.len() + 1);
        tmp.extend_from_slice(bytes);
        tmp.push(0u8);
        // lock the bump to perform the allocation; lock is held only for the allocation duration
        let bump = self.bump.lock().unwrap();
        let vec = bump.alloc_slice_copy(&tmp);
        // create a raw pointer to vec's data
        // SAFETY: we ensure a null terminator is present; the memory will remain valid
        // for the lifetime of the Arena
        let ptr = vec.as_mut_ptr() as *mut c_char;
        ptr as *const c_char
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn arena_alloc_string() {
        let a = Arena::new();
        let p1 = a.alloc_str("hello");
        let p2 = a.alloc_str("hello");
        assert_ne!(p1, std::ptr::null());
        assert_ne!(p2, std::ptr::null());
        unsafe {
            use std::ffi::CStr;
            assert_eq!(CStr::from_ptr(p1).to_str().unwrap(), "hello");
            assert_eq!(CStr::from_ptr(p2).to_str().unwrap(), "hello");
        }
    }

    proptest! {
        #[test]
        fn arena_alloc_property_returns_equal_string(s in any::<String>()) {
            proptest::prop_assume!(!s.contains('\0'));
            let a = Arena::new();
            let _cs = std::ffi::CString::new(s.clone()).unwrap();
            let p = a.alloc_str(&s);
            assert!(!p.is_null());
            unsafe { prop_assert_eq!(std::ffi::CStr::from_ptr(p).to_str().unwrap(), s); }
        }
    }

    #[test]
    fn arena_alloc_concurrent_threads() {
        use std::sync::Arc;
        use std::thread;
        let a = Arc::new(Arena::new());
        let mut handles = vec![];
        for i in 0..8 {
            let arena = a.clone();
            handles.push(thread::spawn(move || {
                let s = format!("concurrent-{}", i);
                let p = arena.alloc_str(&s);
                assert!(!p.is_null());
                unsafe {
                    assert_eq!(std::ffi::CStr::from_ptr(p).to_str().unwrap(), s);
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
    }
}
