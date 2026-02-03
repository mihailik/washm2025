use std::fmt;

/// Representation of a WebAssembly type.
///
/// This matches the `wasm::Type` representation in C++ Binaryen.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Type(u64);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockType {
    Empty,
    Type(Type),
}

impl From<Type> for BlockType {
    fn from(ty: Type) -> Self {
        if ty == Type::NONE {
            BlockType::Empty
        } else {
            BlockType::Type(ty)
        }
    }
}

impl Type {
    // Basic types
    pub const NONE: Type = Type(0);
    pub const UNREACHABLE: Type = Type(1);
    pub const I32: Type = Type(2);
    pub const I64: Type = Type(3);
    pub const F32: Type = Type(4);
    pub const F64: Type = Type(5);
    pub const V128: Type = Type(6);

    // Masks
    const TUPLE_MASK: u64 = 1 << 0;
    const NULL_MASK: u64 = 1 << 1;
    const EXACT_MASK: u64 = 1 << 2;

    // Interned signature flag (bit 32 set indicates this is an interned signature ID)
    const SIGNATURE_FLAG: u64 = 1 << 32;
    // Interned tuple flag (bit 33 set indicates this is an interned tuple ID)
    const TUPLE_FLAG: u64 = 1 << 33;
    const ID_MASK: u64 = 0xFFFF_FFFF; // Lower 32 bits for ID

    // Common Reference Types (Aliases)
    // funcref = nullable func
    pub const FUNCREF: Type = Type((2 << 3) | Self::NULL_MASK);
    // externref = nullable ext
    pub const EXTERNREF: Type = Type((1 << 3) | Self::NULL_MASK);
    // anyref = nullable any
    pub const ANYREF: Type = Type((4 << 3) | Self::NULL_MASK);
    // eqref = nullable eq
    pub const EQREF: Type = Type((5 << 3) | Self::NULL_MASK);
    // i31ref = nullable i31
    pub const I31REF: Type = Type((6 << 3) | Self::NULL_MASK);
    // structref = nullable struct
    pub const STRUCTREF: Type = Type((7 << 3) | Self::NULL_MASK);
    // arrayref = nullable array
    pub const ARRAYREF: Type = Type((8 << 3) | Self::NULL_MASK);

    pub fn new(heap_type: HeapType, nullable: bool) -> Self {
        let null_bit = if nullable { Self::NULL_MASK } else { 0 };
        // Note: Exactness logic omitted for basic constructor for now
        Type(heap_type.0 | null_bit)
    }

    pub fn is_basic(self) -> bool {
        self.0 <= 6
    }

    pub fn is_concrete(self) -> bool {
        self.0 > 1
    }

    pub fn is_nullable(self) -> bool {
        !self.is_basic() && (self.0 & Self::NULL_MASK != 0)
    }

    pub fn is_ref(self) -> bool {
        !self.is_basic() && (self.0 & Self::TUPLE_MASK == 0)
    }

    pub fn get_heap_type(self) -> Option<HeapType> {
        if self.is_ref() {
            // Simplified extraction for basic types
            Some(HeapType(self.0 & !Self::NULL_MASK & !Self::EXACT_MASK))
        } else {
            None
        }
    }

    /// Check if this Type represents an interned signature.
    pub fn is_signature(self) -> bool {
        (self.0 & Self::SIGNATURE_FLAG) != 0
    }

    /// Extract signature ID if this is an interned signature.
    pub fn signature_id(self) -> Option<u32> {
        if self.is_signature() {
            Some((self.0 & Self::ID_MASK) as u32)
        } else {
            None
        }
    }

    /// Create a Type handle from an interned signature ID.
    pub fn from_signature_id(id: u32) -> Self {
        Type(Self::SIGNATURE_FLAG | (id as u64))
    }

    /// Check if this Type represents an interned tuple.
    pub fn is_tuple(self) -> bool {
        (self.0 & Self::TUPLE_FLAG) != 0
    }

    /// Extract tuple ID if this is an interned tuple.
    pub(crate) fn tuple_id(self) -> Option<u32> {
        if self.is_tuple() {
            Some((self.0 & Self::ID_MASK) as u32)
        } else {
            None
        }
    }

    /// Create a Type handle from an interned tuple ID.
    pub(crate) fn from_tuple_id(id: u32) -> Self {
        Type(Self::TUPLE_FLAG | (id as u64))
    }

    pub fn tuple(types: Vec<Type>) -> Self {
        crate::ir::type_store::intern_tuple(types)
    }

    pub fn tuple_len(&self) -> usize {
        if self.is_tuple() {
            if let Some(types) = crate::ir::type_store::try_lookup_tuple(*self) {
                return types.len();
            }
        } else if *self == Self::NONE {
            return 0;
        }
        1
    }

    pub fn tuple_elements(&self) -> Vec<Type> {
        if self.is_tuple() {
            if let Some(types) = crate::ir::type_store::try_lookup_tuple(*self) {
                return types;
            }
        } else if *self == Self::NONE {
            return vec![];
        }
        vec![*self]
    }

    /// Check if this type is a subtype of another type.
    pub fn is_subtype_of(self, other: Type) -> bool {
        if self == other {
            return true;
        }

        if self == Self::UNREACHABLE {
            return true;
        }

        if self.is_basic() || other.is_basic() {
            return self == other;
        }

        // Reference subtyping logic
        if self.is_ref() && other.is_ref() {
            let self_null = self.is_nullable();
            let other_null = other.is_nullable();

            // Nullability: non-null is subtype of nullable
            if self_null && !other_null {
                return false;
            }

            let self_ht = self.get_heap_type().unwrap();
            let other_ht = other.get_heap_type().unwrap();

            return self_ht.is_subtype_of(other_ht);
        }

        // Tuples
        if self.is_tuple() && other.is_tuple() {
            let self_elems = self.tuple_elements();
            let other_elems = other.tuple_elements();

            if self_elems.len() != other_elems.len() {
                return false;
            }

            for (s, o) in self_elems.iter().zip(other_elems.iter()) {
                if !s.is_subtype_of(*o) {
                    return false;
                }
            }
            return true;
        }

        false
    }

    /// Calculate the Least Upper Bound (LUB) of two types.
    pub fn get_lub(a: Type, b: Type) -> Type {
        if a == b {
            return a;
        }
        if a == Self::UNREACHABLE {
            return b;
        }
        if b == Self::UNREACHABLE {
            return a;
        }

        if a.is_basic() || b.is_basic() {
            // No subtyping among basic types
            return if a == b { a } else { Type::ANYREF }; // Fallback for mixed refs/basics? Wasm doesn't allow mixing concrete and refs usually in LUB.
        }

        // Reference LUB
        if a.is_ref() && b.is_ref() {
            let nullable = a.is_nullable() || b.is_nullable();
            let ht_a = a.get_heap_type().unwrap();
            let ht_b = b.get_heap_type().unwrap();
            let lub_ht = HeapType::get_lub(ht_a, ht_b);
            return Type::new(lub_ht, nullable);
        }

        // Tuples
        if a.is_tuple() && b.is_tuple() {
            let a_elems = a.tuple_elements();
            let b_elems = b.tuple_elements();
            if a_elems.len() == b_elems.len() {
                // Return a new tuple of LUBs? This requires interned signatures/tuples access.
                // For now, return a generic type or handle if possible.
            }
        }

        Type::NONE // Or some top type if applicable
    }
}

impl fmt::Debug for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Type::NONE => write!(f, "none"),
            Type::UNREACHABLE => write!(f, "unreachable"),
            Type::I32 => write!(f, "i32"),
            Type::I64 => write!(f, "i64"),
            Type::F32 => write!(f, "f32"),
            Type::F64 => write!(f, "f64"),
            Type::V128 => write!(f, "v128"),
            Type::FUNCREF => write!(f, "funcref"),
            Type::EXTERNREF => write!(f, "externref"),
            Type::ANYREF => write!(f, "anyref"),
            Type::EQREF => write!(f, "eqref"),
            Type::I31REF => write!(f, "i31ref"),
            Type::STRUCTREF => write!(f, "structref"),
            Type::ARRAYREF => write!(f, "arrayref"),
            _ => {
                if self.is_signature() {
                    if let Some(sig) = crate::ir::type_store::try_lookup_signature(*self) {
                        return write!(f, "Signature({:?} -> {:?})", sig.params, sig.results);
                    }
                }
                if self.is_tuple() {
                    if let Some(types) = crate::ir::type_store::try_lookup_tuple(*self) {
                        return write!(f, "Tuple({:?})", types);
                    }
                }
                write!(f, "Type({:#x})", self.0)
            }
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct HeapType(u64);

impl HeapType {
    const _USED_BITS: u64 = 3;

    pub const EXT: HeapType = HeapType(1 << 3);
    pub const FUNC: HeapType = HeapType(2 << 3);
    pub const CONT: HeapType = HeapType(3 << 3);
    pub const ANY: HeapType = HeapType(4 << 3);
    pub const EQ: HeapType = HeapType(5 << 3);
    pub const I31: HeapType = HeapType(6 << 3);
    pub const STRUCT: HeapType = HeapType(7 << 3);
    pub const ARRAY: HeapType = HeapType(8 << 3);
    pub const EXN: HeapType = HeapType(9 << 3);
    pub const STRING: HeapType = HeapType(10 << 3);
    pub const NONE: HeapType = HeapType(11 << 3);
    pub const NOEXT: HeapType = HeapType(12 << 3);
    pub const NOFUNC: HeapType = HeapType(13 << 3);
    pub const NOCONT: HeapType = HeapType(14 << 3);
    pub const NOEXN: HeapType = HeapType(15 << 3);

    pub fn is_basic(self) -> bool {
        // This is a simplification, assuming we only have basic types for now
        true
    }

    pub fn is_subtype_of(self, other: HeapType) -> bool {
        if self == other {
            return true;
        }

        // Basic hierarchy
        // any -> eq -> struct/array/i31
        match other {
            HeapType::ANY => true,
            HeapType::EQ => matches!(
                self,
                HeapType::EQ | HeapType::STRUCT | HeapType::ARRAY | HeapType::I31
            ),
            HeapType::FUNC => self == HeapType::FUNC,
            _ => false,
        }
    }

    pub fn get_lub(a: HeapType, b: HeapType) -> HeapType {
        if a == b {
            return a;
        }

        if a.is_subtype_of(b) {
            return b;
        }
        if b.is_subtype_of(a) {
            return a;
        }

        // Common ancestors
        if a.is_subtype_of(HeapType::EQ) && b.is_subtype_of(HeapType::EQ) {
            return HeapType::EQ;
        }

        HeapType::ANY
    }
}

impl fmt::Debug for HeapType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            HeapType::EXT => write!(f, "ext"),
            HeapType::FUNC => write!(f, "func"),
            HeapType::ANY => write!(f, "any"),
            HeapType::EQ => write!(f, "eq"),
            HeapType::I31 => write!(f, "i31"),
            HeapType::STRUCT => write!(f, "struct"),
            HeapType::ARRAY => write!(f, "array"),
            _ => write!(f, "HeapType({:#x})", self.0),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct Signature {
    pub params: Type,
    pub results: Type,
}

impl Signature {
    pub fn new(params: Type, results: Type) -> Self {
        Self { params, results }
    }
}
