use crate::ir::expression::{ExprRef, ExpressionKind};
use crate::ir::ops::{BinaryOp, UnaryOp};
use crate::ir::Literal;
use bumpalo::Bump;
use std::collections::HashMap;

/// Compile-time expression evaluator
#[allow(dead_code)]
pub struct Evaluator<'a> {
    _arena: &'a Bump,
    /// Known constant values for globals/locals
    /// Using usize for now as GlobalId/LocalId are likely indices
    constants: HashMap<usize, Literal>,
}

impl<'a> Evaluator<'a> {
    pub fn new(arena: &'a Bump) -> Self {
        Self {
            _arena: arena,
            constants: HashMap::new(),
        }
    }

    /// Evaluate expression to constant if possible
    pub fn eval(&self, expr: ExprRef) -> Option<Literal> {
        match &expr.kind {
            ExpressionKind::Const(lit) => Some(lit.clone()),
            ExpressionKind::Binary { op, left, right } => {
                let l = self.eval(*left)?;
                let r = self.eval(*right)?;
                self.eval_binary(*op, l, r)
            }
            ExpressionKind::Unary { op, value } => {
                let v = self.eval(*value)?;
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
            (BinaryOp::LeSInt32, Literal::I32(l), Literal::I32(r)) => {
                Some(Literal::I32(if l <= r { 1 } else { 0 }))
            }
            (BinaryOp::LeUInt32, Literal::I32(l), Literal::I32(r)) => {
                Some(Literal::I32(if (l as u32) <= (r as u32) { 1 } else { 0 }))
            }
            (BinaryOp::GtSInt32, Literal::I32(l), Literal::I32(r)) => {
                Some(Literal::I32(if l > r { 1 } else { 0 }))
            }
            (BinaryOp::GtUInt32, Literal::I32(l), Literal::I32(r)) => {
                Some(Literal::I32(if (l as u32) > (r as u32) { 1 } else { 0 }))
            }
            (BinaryOp::GeSInt32, Literal::I32(l), Literal::I32(r)) => {
                Some(Literal::I32(if l >= r { 1 } else { 0 }))
            }
            (BinaryOp::GeUInt32, Literal::I32(l), Literal::I32(r)) => {
                Some(Literal::I32(if (l as u32) >= (r as u32) { 1 } else { 0 }))
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
                Some(Literal::I64(l.wrapping_shl((r as u32) % 64)))
            }
            (BinaryOp::ShrSInt64, Literal::I64(l), Literal::I64(r)) => {
                Some(Literal::I64(l.wrapping_shr((r as u32) % 64)))
            }
            (BinaryOp::ShrUInt64, Literal::I64(l), Literal::I64(r)) => {
                Some(Literal::I64((l as u64).wrapping_shr((r as u32) % 64) as i64))
            }
            (BinaryOp::RotLInt64, Literal::I64(l), Literal::I64(r)) => {
                Some(Literal::I64(l.rotate_left((r as u32) % 64)))
            }
            (BinaryOp::RotRInt64, Literal::I64(l), Literal::I64(r)) => {
                Some(Literal::I64(l.rotate_right((r as u32) % 64)))
            }
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
            (BinaryOp::LeSInt64, Literal::I64(l), Literal::I64(r)) => {
                Some(Literal::I32(if l <= r { 1 } else { 0 }))
            }
            (BinaryOp::LeUInt64, Literal::I64(l), Literal::I64(r)) => Some(Literal::I32(
                if (u64::from_ne_bytes(l.to_ne_bytes())) <= (u64::from_ne_bytes(r.to_ne_bytes())) {
                    1
                } else {
                    0
                },
            )),
            (BinaryOp::GtSInt64, Literal::I64(l), Literal::I64(r)) => {
                Some(Literal::I32(if l > r { 1 } else { 0 }))
            }
            (BinaryOp::GtUInt64, Literal::I64(l), Literal::I64(r)) => {
                Some(Literal::I32(if (l as u64) > (r as u64) { 1 } else { 0 }))
            }
            (BinaryOp::GeSInt64, Literal::I64(l), Literal::I64(r)) => {
                Some(Literal::I32(if l >= r { 1 } else { 0 }))
            }
            (BinaryOp::GeUInt64, Literal::I64(l), Literal::I64(r)) => {
                Some(Literal::I32(if (l as u64) >= (r as u64) { 1 } else { 0 }))
            }

            // TODO: Implement f32, f64 ops
            _ => None,
        }
    }

    fn eval_unary(&self, op: UnaryOp, v: Literal) -> Option<Literal> {
        match (op, v) {
            // Int32
            (UnaryOp::EqZInt32, Literal::I32(v)) => Some(Literal::I32(if v == 0 { 1 } else { 0 })),
            (UnaryOp::ClzInt32, Literal::I32(v)) => Some(Literal::I32(v.leading_zeros() as i32)),
            (UnaryOp::CtzInt32, Literal::I32(v)) => Some(Literal::I32(v.trailing_zeros() as i32)),
            (UnaryOp::PopcntInt32, Literal::I32(v)) => Some(Literal::I32(v.count_ones() as i32)),

            // Int64
            (UnaryOp::EqZInt64, Literal::I64(v)) => Some(Literal::I32(if v == 0 { 1 } else { 0 })),
            (UnaryOp::ClzInt64, Literal::I64(v)) => Some(Literal::I64(v.leading_zeros() as i64)),
            (UnaryOp::CtzInt64, Literal::I64(v)) => Some(Literal::I64(v.trailing_zeros() as i64)),
            (UnaryOp::PopcntInt64, Literal::I64(v)) => Some(Literal::I64(v.count_ones() as i64)),

            // Conversions
            (UnaryOp::ExtendSInt32, Literal::I32(v)) => Some(Literal::I64(v as i64)),
            (UnaryOp::ExtendUInt32, Literal::I32(v)) => Some(Literal::I64(v as u32 as i64)),
            (UnaryOp::WrapInt64, Literal::I64(v)) => Some(Literal::I32(v as i32)),

            (UnaryOp::ExtendS8Int32, Literal::I32(v)) => Some(Literal::I32(v as i8 as i32)),
            (UnaryOp::ExtendS16Int32, Literal::I32(v)) => Some(Literal::I32(v as i16 as i32)),
            (UnaryOp::ExtendS8Int64, Literal::I64(v)) => Some(Literal::I64(v as i8 as i64)),
            (UnaryOp::ExtendS16Int64, Literal::I64(v)) => Some(Literal::I64(v as i16 as i64)),
            (UnaryOp::ExtendS32Int64, Literal::I64(v)) => Some(Literal::I64(v as i32 as i64)),

            // TODO: Implement f32, f64 ops
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::IrBuilder;
    use crate::ir::Type;

    #[test]
    fn test_eval_basic() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let evaluator = Evaluator::new(&bump);

        // 1 + 2 = 3
        let c1 = builder.const_(Literal::I32(1));
        let c2 = builder.const_(Literal::I32(2));
        let add = builder.binary(BinaryOp::AddInt32, c1, c2, Type::I32);

        assert_eq!(evaluator.eval(add), Some(Literal::I32(3)));
    }

    #[test]
    fn test_eval_nested() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let evaluator = Evaluator::new(&bump);

        // (1 + 2) * 3 = 9
        let c1 = builder.const_(Literal::I32(1));
        let c2 = builder.const_(Literal::I32(2));
        let add = builder.binary(BinaryOp::AddInt32, c1, c2, Type::I32);
        let c3 = builder.const_(Literal::I32(3));
        let mul = builder.binary(BinaryOp::MulInt32, add, c3, Type::I32);

        assert_eq!(evaluator.eval(mul), Some(Literal::I32(9)));
    }

    #[test]
    fn test_eval_div_zero() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let evaluator = Evaluator::new(&bump);

        // 1 / 0 = None
        let c1 = builder.const_(Literal::I32(1));
        let c0 = builder.const_(Literal::I32(0));
        let div = builder.binary(BinaryOp::DivSInt32, c1, c0, Type::I32);

        assert_eq!(evaluator.eval(div), None);
    }

    #[test]
    fn test_eval_i64() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let evaluator = Evaluator::new(&bump);

        // (1 << 33) | 7 = 0x200000007
        let c1 = builder.const_(Literal::I64(1));
        let c33 = builder.const_(Literal::I64(33));
        let shl = builder.binary(BinaryOp::ShlInt64, c1, c33, Type::I64);
        let c7 = builder.const_(Literal::I64(7));
        let or = builder.binary(BinaryOp::OrInt64, shl, c7, Type::I64);

        assert_eq!(evaluator.eval(or), Some(Literal::I64((1i64 << 33) | 7)));
    }

    #[test]
    fn test_eval_conversions() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let evaluator = Evaluator::new(&bump);

        // extend_s/i32 (-1) -> -1i64
        let minus_one = builder.const_(Literal::I32(-1));
        let extend = builder.unary(UnaryOp::ExtendSInt32, minus_one, Type::I64);
        assert_eq!(evaluator.eval(extend), Some(Literal::I64(-1)));

        // extend_u/i32 (-1) -> 0xffffffffi64
        let extend_u = builder.unary(UnaryOp::ExtendUInt32, minus_one, Type::I64);
        assert_eq!(evaluator.eval(extend_u), Some(Literal::I64(0xffffffff)));
    }
}
