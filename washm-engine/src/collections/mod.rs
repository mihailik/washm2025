// Minimal support crate skeleton

pub fn version() -> &'static str {
    "binaryen-support-0.1"
}

pub mod strings;
pub use strings::StringInterner;
pub mod arena;
pub use arena::Arena;
pub mod id_arena;
pub use id_arena::{Id, IdArena};
pub mod hash;
pub use hash::FastHashMap;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn it_works() {
        assert_eq!(version(), "binaryen-support-0.1");
    }
}
