use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::ops::{BinaryOp, UnaryOp};
use crate::ir::Literal;
use std::collections::HashMap;

/// Compile-time expression evaluator
#[allow(dead_code)]
#[derive(Default)]
pub struct Evaluator {
    /// Known constant values for globals/locals
    /// Using usize for now as GlobalId/LocalId are likely indices
    constants: HashMap<usize, Literal>,
}

impl Evaluator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Evaluate expression to constant if possible
    pub fn eval(
        &self,
        expr_id: ExpressionId,
        arena: &IdArena<ExpressionMarker, Expression>,
    ) -> Option<Literal> {
        match &arena[expr_id].kind {
            ExpressionKind::Const(lit) => Some(*lit),
            ExpressionKind::Binary { op, left, right } => {
                let l = self.eval(*left, arena)?;
                let r = self.eval(*right, arena)?;
                self.eval_binary(*op, l, r)
            }
            ExpressionKind::Unary { op, value } => {
                let v = self.eval(*value, arena)?;
                self.eval_unary(*op, v)
            }
            // TODO: Handle GlobalGet/LocalGet if we have context
            _ => None,
        }
    }

    fn eval_binary(&self, op: BinaryOp, l: Literal, r: Literal) -> Option<Literal> {
        match (op, l, r) {
            // Int32
            (BinaryOp::AddInt32, Literal::I32(l), Literal::I32(r)) => {
                Some(Literal::I32(l.wrapping_add(r)))
            }
            (BinaryOp::SubInt32, Literal::I32(l), Literal::I32(r)) => {
                Some(Literal::I32(l.wrapping_sub(r)))
            }
            (BinaryOp::MulInt32, Literal::I32(l), Literal::I32(r)) => {
                Some(Literal::I32(l.wrapping_mul(r)))
            }
            (BinaryOp::DivSInt32, Literal::I32(l), Literal::I32(r)) => {
                if r == 0 {
                    return None;
                } // Division by zero
                if l == i32::MIN && r == -1 {
                    return None;
                } // Overflow
                Some(Literal::I32(l.wrapping_div(r)))
            }
            (BinaryOp::DivUInt32, Literal::I32(l), Literal::I32(r)) => {
                if r == 0 {
                    return None;
                }
                Some(Literal::I32((l as u32).wrapping_div(r as u32) as i32))
            }
            (BinaryOp::RemSInt32, Literal::I32(l), Literal::I32(r)) => {
                if r == 0 {
                    return None;
                }
                if l == i32::MIN && r == -1 {
                    return Some(Literal::I32(0));
                }
                Some(Literal::I32(l.wrapping_rem(r)))
            }
            (BinaryOp::RemUInt32, Literal::I32(l), Literal::I32(r)) => {
                if r == 0 {
                    return None;
                }
                Some(Literal::I32((l as u32).wrapping_rem(r as u32) as i32))
            }
            (BinaryOp::AndInt32, Literal::I32(l), Literal::I32(r)) => Some(Literal::I32(l & r)),
            (BinaryOp::OrInt32, Literal::I32(l), Literal::I32(r)) => Some(Literal::I32(l | r)),
            (BinaryOp::XorInt32, Literal::I32(l), Literal::I32(r)) => Some(Literal::I32(l ^ r)),
            (BinaryOp::ShlInt32, Literal::I32(l), Literal::I32(r)) => {
                // WASM shift amount is taken modulo 32
                Some(Literal::I32(l.wrapping_shl((r as u32) % 32)))
            }
            (BinaryOp::ShrSInt32, Literal::I32(l), Literal::I32(r)) => {
                Some(Literal::I32(l.wrapping_shr((r as u32) % 32)))
            }
            (BinaryOp::ShrUInt32, Literal::I32(l), Literal::I32(r)) => {
                Some(Literal::I32((l as u32).wrapping_shr((r as u32) % 32) as i32))
            }
            (BinaryOp::RotLInt32, Literal::I32(l), Literal::I32(r)) => {
                Some(Literal::I32(l.rotate_left((r as u32) % 32)))
            }
            (BinaryOp::RotRInt32, Literal::I32(l), Literal::I32(r)) => {
                Some(Literal::I32(l.rotate_right((r as u32) % 32)))
            }

            // Int64
            (BinaryOp::AddInt64, Literal::I64(l), Literal::I64(r)) => {
                Some(Literal::I64(l.wrapping_add(r)))
            }
            (BinaryOp::SubInt64, Literal::I64(l), Literal::I64(r)) => {
                Some(Literal::I64(l.wrapping_sub(r)))
            }
            (BinaryOp::MulInt64, Literal::I64(l), Literal::I64(r)) => {
                Some(Literal::I64(l.wrapping_mul(r)))
            }
            (BinaryOp::DivSInt64, Literal::I64(l), Literal::I64(r)) => {
                if r == 0 {
                    return None;
                }
                if l == i64::MIN && r == -1 {
                    return None;
                }
                Some(Literal::I64(l.wrapping_div(r)))
            }
            (BinaryOp::DivUInt64, Literal::I64(l), Literal::I64(r)) => {
                if r == 0 {
                    return None;
                }
                Some(Literal::I64((l as u64).wrapping_div(r as u64) as i64))
            }
            (BinaryOp::RemSInt64, Literal::I64(l), Literal::I64(r)) => {
                if r == 0 {
                    return None;
                }
                if l == i64::MIN && r == -1 {
                    return Some(Literal::I64(0));
                }
                Some(Literal::I64(l.wrapping_rem(r)))
            }
            (BinaryOp::RemUInt64, Literal::I64(l), Literal::I64(r)) => {
                if r == 0 {
                    return None;
                }
                Some(Literal::I64((l as u64).wrapping_rem(r as u64) as i64))
            }
            (BinaryOp::AndInt64, Literal::I64(l), Literal::I64(r)) => Some(Literal::I64(l & r)),
            (BinaryOp::OrInt64, Literal::I64(l), Literal::I64(r)) => Some(Literal::I64(l | r)),
            (BinaryOp::XorInt64, Literal::I64(l), Literal::I64(r)) => Some(Literal::I64(l ^ r)),
            (BinaryOp::ShlInt64, Literal::I64(l), Literal::I64(r)) => {
                Some(Literal::I64(l.wrapping_shl((r as u64 % 64) as u32)))
            }
            (BinaryOp::ShrSInt64, Literal::I64(l), Literal::I64(r)) => {
                Some(Literal::I64(l.wrapping_shr((r as u64 % 64) as u32)))
            }
            (BinaryOp::ShrUInt64, Literal::I64(l), Literal::I64(r)) => Some(Literal::I64(
                (l as u64).wrapping_shr((r as u64 % 64) as u32) as i64,
            )),
            (BinaryOp::RotLInt64, Literal::I64(l), Literal::I64(r)) => {
                Some(Literal::I64(l.rotate_left((r as u64 % 64) as u32)))
            }
            (BinaryOp::RotRInt64, Literal::I64(l), Literal::I64(r)) => {
                Some(Literal::I64(l.rotate_right((r as u64 % 64) as u32)))
            }

            // Comparisons I32
            (BinaryOp::EqInt32, Literal::I32(l), Literal::I32(r)) => {
                Some(Literal::I32(if l == r { 1 } else { 0 }))
            }
            (BinaryOp::NeInt32, Literal::I32(l), Literal::I32(r)) => {
                Some(Literal::I32(if l != r { 1 } else { 0 }))
            }
            (BinaryOp::LtSInt32, Literal::I32(l), Literal::I32(r)) => {
                Some(Literal::I32(if l < r { 1 } else { 0 }))
            }
            (BinaryOp::LtUInt32, Literal::I32(l), Literal::I32(r)) => {
                Some(Literal::I32(if (l as u32) < (r as u32) { 1 } else { 0 }))
            }
            (BinaryOp::GtSInt32, Literal::I32(l), Literal::I32(r)) => {
                Some(Literal::I32(if l > r { 1 } else { 0 }))
            }
            (BinaryOp::GtUInt32, Literal::I32(l), Literal::I32(r)) => {
                Some(Literal::I32(if (l as u32) > (r as u32) { 1 } else { 0 }))
            }
            (BinaryOp::LeSInt32, Literal::I32(l), Literal::I32(r)) => {
                Some(Literal::I32(if l <= r { 1 } else { 0 }))
            }
            (BinaryOp::LeUInt32, Literal::I32(l), Literal::I32(r)) => {
                Some(Literal::I32(if (l as u32) <= (r as u32) { 1 } else { 0 }))
            }
            (BinaryOp::GeSInt32, Literal::I32(l), Literal::I32(r)) => {
                Some(Literal::I32(if l >= r { 1 } else { 0 }))
            }
            (BinaryOp::GeUInt32, Literal::I32(l), Literal::I32(r)) => {
                Some(Literal::I32(if (l as u32) >= (r as u32) { 1 } else { 0 }))
            }

            // Comparisons I64
            (BinaryOp::EqInt64, Literal::I64(l), Literal::I64(r)) => {
                Some(Literal::I32(if l == r { 1 } else { 0 }))
            }
            (BinaryOp::NeInt64, Literal::I64(l), Literal::I64(r)) => {
                Some(Literal::I32(if l != r { 1 } else { 0 }))
            }
            (BinaryOp::LtSInt64, Literal::I64(l), Literal::I64(r)) => {
                Some(Literal::I32(if l < r { 1 } else { 0 }))
            }
            (BinaryOp::LtUInt64, Literal::I64(l), Literal::I64(r)) => {
                Some(Literal::I32(if (l as u64) < (r as u64) { 1 } else { 0 }))
            }
            (BinaryOp::GtSInt64, Literal::I64(l), Literal::I64(r)) => {
                Some(Literal::I32(if l > r { 1 } else { 0 }))
            }
            (BinaryOp::GtUInt64, Literal::I64(l), Literal::I64(r)) => {
                Some(Literal::I32(if (l as u64) > (r as u64) { 1 } else { 0 }))
            }
            (BinaryOp::LeSInt64, Literal::I64(l), Literal::I64(r)) => {
                Some(Literal::I32(if l <= r { 1 } else { 0 }))
            }
            (BinaryOp::LeUInt64, Literal::I64(l), Literal::I64(r)) => {
                Some(Literal::I32(if (l as u64) <= (r as u64) { 1 } else { 0 }))
            }
            (BinaryOp::GeSInt64, Literal::I64(l), Literal::I64(r)) => {
                Some(Literal::I32(if l >= r { 1 } else { 0 }))
            }
            (BinaryOp::GeUInt64, Literal::I64(l), Literal::I64(r)) => {
                Some(Literal::I32(if (l as u64) >= (r as u64) { 1 } else { 0 }))
            }

            // Float 32
            (BinaryOp::AddFloat32, Literal::F32(l), Literal::F32(r)) => Some(Literal::F32(l + r)),
            (BinaryOp::SubFloat32, Literal::F32(l), Literal::F32(r)) => Some(Literal::F32(l - r)),
            (BinaryOp::MulFloat32, Literal::F32(l), Literal::F32(r)) => Some(Literal::F32(l * r)),
            (BinaryOp::DivFloat32, Literal::F32(l), Literal::F32(r)) => Some(Literal::F32(l / r)),
            (BinaryOp::MinFloat32, Literal::F32(l), Literal::F32(r)) => {
                Some(Literal::F32(l.min(r)))
            }
            (BinaryOp::MaxFloat32, Literal::F32(l), Literal::F32(r)) => {
                Some(Literal::F32(l.max(r)))
            }

            // Float 64
            (BinaryOp::AddFloat64, Literal::F64(l), Literal::F64(r)) => Some(Literal::F64(l + r)),
            (BinaryOp::SubFloat64, Literal::F64(l), Literal::F64(r)) => Some(Literal::F64(l - r)),
            (BinaryOp::MulFloat64, Literal::F64(l), Literal::F64(r)) => Some(Literal::F64(l * r)),
            (BinaryOp::DivFloat64, Literal::F64(l), Literal::F64(r)) => Some(Literal::F64(l / r)),
            (BinaryOp::MinFloat64, Literal::F64(l), Literal::F64(r)) => {
                Some(Literal::F64(l.min(r)))
            }
            (BinaryOp::MaxFloat64, Literal::F64(l), Literal::F64(r)) => {
                Some(Literal::F64(l.max(r)))
            }

            _ => None,
        }
    }

    fn eval_unary(&self, op: UnaryOp, v: Literal) -> Option<Literal> {
        match (op, v) {
            (UnaryOp::EqZInt32, Literal::I32(v)) => Some(Literal::I32(if v == 0 { 1 } else { 0 })),
            (UnaryOp::ClzInt32, Literal::I32(v)) => Some(Literal::I32(v.leading_zeros() as i32)),
            (UnaryOp::CtzInt32, Literal::I32(v)) => Some(Literal::I32(v.trailing_zeros() as i32)),
            (UnaryOp::PopcntInt32, Literal::I32(v)) => Some(Literal::I32(v.count_ones() as i32)),

            (UnaryOp::EqZInt64, Literal::I64(v)) => Some(Literal::I32(if v == 0 { 1 } else { 0 })),
            (UnaryOp::ClzInt64, Literal::I64(v)) => Some(Literal::I32(v.leading_zeros() as i32)),
            (UnaryOp::CtzInt64, Literal::I64(v)) => Some(Literal::I32(v.trailing_zeros() as i32)),
            (UnaryOp::PopcntInt64, Literal::I64(v)) => Some(Literal::I32(v.count_ones() as i32)),

            (UnaryOp::AbsFloat32, Literal::F32(v)) => Some(Literal::F32(v.abs())),
            (UnaryOp::NegFloat32, Literal::F32(v)) => Some(Literal::F32(-v)),
            (UnaryOp::CeilFloat32, Literal::F32(v)) => Some(Literal::F32(v.ceil())),
            (UnaryOp::FloorFloat32, Literal::F32(v)) => Some(Literal::F32(v.floor())),
            (UnaryOp::TruncFloat32, Literal::F32(v)) => Some(Literal::F32(v.trunc())),
            (UnaryOp::NearestFloat32, Literal::F32(v)) => Some(Literal::F32(v.round())),
            (UnaryOp::SqrtFloat32, Literal::F32(v)) => Some(Literal::F32(v.sqrt())),

            (UnaryOp::AbsFloat64, Literal::F64(v)) => Some(Literal::F64(v.abs())),
            (UnaryOp::NegFloat64, Literal::F64(v)) => Some(Literal::F64(-v)),
            (UnaryOp::CeilFloat64, Literal::F64(v)) => Some(Literal::F64(v.ceil())),
            (UnaryOp::FloorFloat64, Literal::F64(v)) => Some(Literal::F64(v.floor())),
            (UnaryOp::TruncFloat64, Literal::F64(v)) => Some(Literal::F64(v.trunc())),
            (UnaryOp::NearestFloat64, Literal::F64(v)) => Some(Literal::F64(v.round())),
            (UnaryOp::SqrtFloat64, Literal::F64(v)) => Some(Literal::F64(v.sqrt())),

            // Conversions
            (UnaryOp::ExtendSInt32, Literal::I32(v)) => Some(Literal::I64(v as i64)),
            (UnaryOp::ExtendUInt32, Literal::I32(v)) => Some(Literal::I64(v as u32 as i64)),
            (UnaryOp::WrapInt64, Literal::I64(v)) => Some(Literal::I32(v as i32)),

            _ => None,
        }
    }
}
