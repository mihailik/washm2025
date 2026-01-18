mod r#type;
pub use r#type::*;

mod literal;
pub use literal::*;

pub mod annotation;
pub mod expression;
pub mod module;
pub mod ops;
pub mod visitor;
pub mod wasm_features;

pub mod type_store;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_name() {
        assert_eq!(Type::I32.to_string(), "i32");
        assert_eq!(Type::F64.to_string(), "f64");
    }

    #[test]
    fn test_type_equality() {
        assert_eq!(Type::I32, Type::I32);
        assert_ne!(Type::I32, Type::I64);
    }

    #[test]
    fn test_ref_types() {
        assert!(Type::FUNCREF.is_ref());
        assert!(Type::FUNCREF.is_nullable());
        assert!(!Type::I32.is_ref());
        assert!(!Type::I32.is_nullable());

        let func_heap_type = Type::FUNCREF.get_heap_type().unwrap();
        assert_eq!(func_heap_type, HeapType::FUNC);
    }

    #[test]
    fn test_signature() {
        let sig = Signature::new(Type::I32, Type::F64);
        assert_eq!(sig.params, Type::I32);
        assert_eq!(sig.results, Type::F64);
    }
}
