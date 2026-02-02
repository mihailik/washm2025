//! Global type store for interning complex type definitions.
//!
//! This module manages canonicalization of `Signature` and `HeapType` definitions,
//! ensuring that equivalent types map to identical `Type` handles across the system.

use crate::ir::r#type::{Signature, Type};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::RwLock;

/// Central registry for interning type definitions.
///
/// Thread-safe via `RwLock`. May be upgraded to `dashmap` or sharded storage
/// if lock contention becomes measurable under profiling.
pub struct TypeStore {
    /// Map from (params, results) to canonical Type ID
    signatures: HashMap<(Type, Type), u32>,
    /// Reverse lookup: Type ID -> Signature definition
    rev_signatures: HashMap<u32, Signature>,
    /// Map from list of types to canonical Type ID (Tuples)
    tuples: HashMap<Vec<Type>, u32>,
    /// Reverse lookup for tuples
    rev_tuples: HashMap<u32, Vec<Type>>,
    /// Counter for generating unique signature IDs
    next_sig_id: u32,
}

impl Default for TypeStore {
    fn default() -> Self {
        Self {
            signatures: HashMap::new(),
            rev_signatures: HashMap::new(),
            tuples: HashMap::new(),
            rev_tuples: HashMap::new(),
            // Start IDs high to distinguish from basic type range (0..255)
            next_sig_id: 0x1000,
        }
    }
}

impl TypeStore {
    /// Intern a tuple of types.
    fn intern_tuple_impl(&mut self, types: Vec<Type>) -> Type {
        if types.is_empty() {
            return Type::NONE;
        }
        if types.len() == 1 {
            return types[0];
        }

        if let Some(&id) = self.tuples.get(&types) {
            return Type::from_tuple_id(id);
        }

        let id = self.next_sig_id;
        self.next_sig_id += 1;

        self.tuples.insert(types.clone(), id);
        self.rev_tuples.insert(id, types);

        Type::from_tuple_id(id)
    }

    /// Intern a signature, returning a canonical Type handle.
    ///
    /// If this (params, results) pair was previously interned, returns the existing ID.
    /// Otherwise, allocates a new ID and stores the mapping.
    fn intern_signature_impl(&mut self, params: Type, results: Type) -> Type {
        let key = (params, results);

        if let Some(&id) = self.signatures.get(&key) {
            // Already interned - return existing handle
            return Type::from_signature_id(id);
        }

        // Allocate new ID
        let id = self.next_sig_id;
        self.next_sig_id += 1;

        let sig = Signature::new(params, results);
        self.signatures.insert(key, id);
        self.rev_signatures.insert(id, sig);

        Type::from_signature_id(id)
    }

    /// Lookup a signature by Type handle.
    ///
    /// Returns `None` if the Type is not an interned signature.
    fn lookup_signature_impl(&self, ty: Type) -> Option<Signature> {
        let id = ty.signature_id()?;
        self.rev_signatures.get(&id).copied()
    }

    fn lookup_tuple_impl(&self, ty: Type) -> Option<Vec<Type>> {
        let id = ty.tuple_id()?;
        self.rev_tuples.get(&id).cloned()
    }
}

/// Global singleton TypeStore.
///
/// Access via `intern_signature()` and `lookup_signature()` functions.
static TYPE_STORE: Lazy<RwLock<TypeStore>> = Lazy::new(|| RwLock::new(TypeStore::default()));

/// Intern a signature into the global type store.
///
/// This is the primary API for creating function type handles.
///
/// # Thread Safety
/// Acquires a write lock. Multiple threads creating the same signature
/// will correctly receive identical handles due to interning.
pub fn intern_signature(params: Type, results: Type) -> Type {
    let mut store = TYPE_STORE.write().unwrap();
    store.intern_signature_impl(params, results)
}

/// Intern a tuple into the global type store.
pub fn intern_tuple(types: Vec<Type>) -> Type {
    let mut store = TYPE_STORE.write().unwrap();
    store.intern_tuple_impl(types)
}

/// Look up a signature definition from an interned Type handle.
///
/// Returns `None` if the Type is not an interned signature or the ID is invalid.
pub fn lookup_signature(ty: Type) -> Option<Signature> {
    let store = TYPE_STORE.read().unwrap();
    store.lookup_signature_impl(ty)
}

/// Look up a tuple definition from an interned Type handle.
pub fn lookup_tuple(ty: Type) -> Option<Vec<Type>> {
    let store = TYPE_STORE.read().unwrap();
    store.lookup_tuple_impl(ty)
}

/// Attempts to lookup a tuple without blocking.
/// Useful for Debug/Display implementations to avoid deadlocks.
pub fn try_lookup_tuple(ty: Type) -> Option<Vec<Type>> {
    if let Ok(store) = TYPE_STORE.try_read() {
        store.lookup_tuple_impl(ty)
    } else {
        None
    }
}

/// Attempts to lookup a signature without blocking.
/// Useful for Debug/Display implementations to avoid deadlocks.
pub fn try_lookup_signature(ty: Type) -> Option<Signature> {
    if let Ok(store) = TYPE_STORE.try_read() {
        store.lookup_signature_impl(ty)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intern_tuple_lookup() {
        let types = vec![Type::I32, Type::I32];
        let ty = intern_tuple(types.clone());
        let looked_up = lookup_tuple(ty).expect("Should find interned tuple");
        assert_eq!(types, looked_up);
    }

    #[test]
    fn test_intern_different_signatures() {
        let ty1 = intern_signature(Type::I32, Type::I64);
        let ty2 = intern_signature(Type::F32, Type::F64);
        assert_ne!(ty1, ty2, "Different signatures should have different IDs");
    }

    #[test]
    fn test_lookup_interned_signature() {
        let ty = intern_signature(Type::I32, Type::F64);
        let sig = lookup_signature(ty).expect("Should find interned signature");
        assert_eq!(sig.params, Type::I32);
        assert_eq!(sig.results, Type::F64);
    }

    #[test]
    fn test_lookup_basic_type_returns_none() {
        let sig = lookup_signature(Type::I32);
        assert!(sig.is_none(), "Basic types are not signatures");
    }
}
