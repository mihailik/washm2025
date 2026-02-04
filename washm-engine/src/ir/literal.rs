use crate::ir::Type;
use std::fmt;
use std::ops::Neg;

#[derive(Clone, Copy, PartialEq)]
pub enum Literal {
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    V128([u8; 16]),
}

impl Eq for Literal {}

impl Literal {
    pub fn get_type(&self) -> Type {
        match self {
            Literal::I32(_) => Type::I32,
            Literal::I64(_) => Type::I64,
            Literal::F32(_) => Type::F32,
            Literal::F64(_) => Type::F64,
            Literal::V128(_) => Type::V128,
        }
    }

    pub fn get_i32(&self) -> i32 {
        if let Literal::I32(v) = self {
            *v
        } else {
            panic!("not an i32 literal");
        }
    }

    pub fn get_u32(&self) -> u32 {
        if let Literal::I32(v) = self {
            *v as u32
        } else {
            panic!("not an i32 literal");
        }
    }

    pub fn get_i64(&self) -> i64 {
        if let Literal::I64(v) = self {
            *v
        } else {
            panic!("not an i64 literal");
        }
    }

    pub fn get_u64(&self) -> u64 {
        if let Literal::I64(v) = self {
            *v as u64
        } else {
            panic!("not an i64 literal");
        }
    }

    pub fn get_f32(&self) -> f32 {
        if let Literal::F32(v) = self {
            *v
        } else {
            panic!("not an f32 literal");
        }
    }

    pub fn get_f64(&self) -> f64 {
        if let Literal::F64(v) = self {
            *v
        } else {
            panic!("not an f64 literal");
        }
    }
}

impl Neg for Literal {
    type Output = Literal;

    fn neg(self) -> Self::Output {
        match self {
            Literal::I32(v) => Literal::I32(-v),
            Literal::I64(v) => Literal::I64(-v),
            Literal::F32(v) => Literal::F32(-v),
            Literal::F64(v) => Literal::F64(-v),
            Literal::V128(_) => panic!("V128 literals cannot be negated"),
        }
    }
}

impl fmt::Debug for Literal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Literal::I32(v) => write!(f, "i32.const {}", v),
            Literal::I64(v) => write!(f, "i64.const {}", v),
            Literal::F32(v) => write!(f, "f32.const {}", v),
            Literal::F64(v) => write!(f, "f64.const {}", v),
            Literal::V128(v) => write!(f, "v128.const {:?}", v),
        }
    }
}

impl fmt::Display for Literal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Literal::I32(v) => write!(f, "{}", v),
            Literal::I64(v) => write!(f, "{}", v),
            Literal::F32(v) => write!(f, "{}", v),
            Literal::F64(v) => write!(f, "{}", v),
            Literal::V128(v) => write!(f, "v128.const {:02x?}", v),
        }
    }
}
