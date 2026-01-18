use crate::ir::expression::{ExprRef, ExpressionKind};
use crate::ir::module::Module;
use crate::ir::ops::{BinaryOp, RefAsOp, RefCastOp, UnaryOp};
use crate::passes::Pass;
use crate::ir::visitor::Visitor;
use crate::ir::Literal;

pub struct SimplifyIdentity;

impl Pass for SimplifyIdentity {
    fn name(&self) -> &str {
        "SimplifyIdentity"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        for func in &mut module.functions {
            if let Some(body) = &mut func.body {
                self.visit(body);
            }
        }
    }
}

fn is_zero(expr: &ExprRef) -> bool {
    matches!(
        expr.kind,
        ExpressionKind::Const(Literal::I32(0)) | ExpressionKind::Const(Literal::I64(0))
    )
}

fn is_one(expr: &ExprRef) -> bool {
    matches!(
        expr.kind,
        ExpressionKind::Const(Literal::I32(1)) | ExpressionKind::Const(Literal::I64(1))
    )
}

fn is_all_ones(expr: &ExprRef) -> bool {
    matches!(
        expr.kind,
        ExpressionKind::Const(Literal::I32(-1)) | ExpressionKind::Const(Literal::I64(-1))
    )
}

fn are_expressions_equal(a: &ExprRef, b: &ExprRef) -> bool {
    // Very basic equality check for constants
    match (&a.kind, &b.kind) {
        (ExpressionKind::Const(la), ExpressionKind::Const(lb)) => la == lb,
        (ExpressionKind::LocalGet { index: ia }, ExpressionKind::LocalGet { index: ib }) => {
            ia == ib
        }
        _ => false, // Fallback
    }
}

impl<'a> Visitor<'a> for SimplifyIdentity {
    fn visit(&mut self, expr: &mut ExprRef<'a>) {
        // Post-order: children first, then parent
        self.visit_children(expr);
        self.visit_expression(expr);
    }

    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        // Recursive identity pull-up
        match &mut expr.kind {
            ExpressionKind::Unary {
                op: UnaryOp::PopcntInt32,
                value,
            }
            | ExpressionKind::Unary {
                op: UnaryOp::ClzInt32,
                value,
            }
            | ExpressionKind::Unary {
                op: UnaryOp::CtzInt32,
                value,
            } => {
                if let ExpressionKind::Const(Literal::I32(v)) = value.kind {
                    let result = match expr.kind {
                        ExpressionKind::Unary {
                            op: UnaryOp::PopcntInt32,
                            ..
                        } => v.count_ones() as i32,
                        ExpressionKind::Unary {
                            op: UnaryOp::ClzInt32,
                            ..
                        } => v.leading_zeros() as i32,
                        ExpressionKind::Unary {
                            op: UnaryOp::CtzInt32,
                            ..
                        } => v.trailing_zeros() as i32,
                        _ => unreachable!(),
                    };
                    expr.kind = ExpressionKind::Const(Literal::I32(result));
                    return;
                }
            }
            _ => {}
        }

        // Optimization for EqZ(EqZ(EqZ(x))) -> EqZ(x)
        if let ExpressionKind::Unary { op: op1, value: v1 } = &mut expr.kind {
            if matches!(op1, UnaryOp::EqZInt32 | UnaryOp::EqZInt64) {
                if let ExpressionKind::Unary { op: op2, value: v2 } = &mut v1.kind {
                    if matches!(op2, UnaryOp::EqZInt32 | UnaryOp::EqZInt64) {
                        if let ExpressionKind::Unary { op: op3, value: v3 } = &mut v2.kind {
                            if matches!(op3, UnaryOp::EqZInt32 | UnaryOp::EqZInt64) {
                                *v1 = *v3;
                            }
                        }
                    }
                }
            }
        }

        match &mut expr.kind {
            ExpressionKind::Binary { op, left, right } => {
                #[derive(Clone, Copy, PartialEq, Eq)]
                enum IdentitySide {
                    Left,
                    Right,
                    Both, // For x op x
                    None,
                }

                let side = match op {
                    // x & 0 -> 0, 0 & x -> 0
                    BinaryOp::AndInt32 | BinaryOp::AndInt64 if is_zero(left) || is_zero(right) => {
                        let is_64 = matches!(op, BinaryOp::AndInt64);
                        expr.kind = ExpressionKind::Const(if is_64 { Literal::I64(0) } else { Literal::I32(0) });
                        return;
                    }
                    // x | -1 -> -1, -1 | x -> -1
                    BinaryOp::OrInt32 if is_all_ones(left) || is_all_ones(right) => {
                        expr.kind = ExpressionKind::Const(Literal::I32(-1));
                        return;
                    }
                    BinaryOp::OrInt64 if is_all_ones(left) || is_all_ones(right) => {
                        expr.kind = ExpressionKind::Const(Literal::I64(-1));
                        return;
                    }
                    // x + 0 -> x, 0 + x -> x
                    BinaryOp::AddInt32 | BinaryOp::AddInt64 |
                    // x | 0 -> x, 0 | x -> x
                    BinaryOp::OrInt32 | BinaryOp::OrInt64 |
                    // x ^ 0 -> x, 0 ^ x -> x
                    BinaryOp::XorInt32 | BinaryOp::XorInt64 => {
                        if is_zero(right) {
                            IdentitySide::Right
                        } else if is_zero(left) {
                            IdentitySide::Left
                        } else {
                            IdentitySide::None
                        }
                    }
                    // x - 0 -> x
                    BinaryOp::SubInt32 | BinaryOp::SubInt64 |
                    BinaryOp::ShlInt32 | BinaryOp::ShlInt64 |
                    BinaryOp::ShrSInt32 | BinaryOp::ShrSInt64 |
                    BinaryOp::ShrUInt32 | BinaryOp::ShrUInt64 |
                    BinaryOp::RotLInt32 | BinaryOp::RotLInt64 |
                    BinaryOp::RotRInt32 | BinaryOp::RotRInt64 => {
                        if is_zero(right) {
                            IdentitySide::Right
                        } else {
                            IdentitySide::None
                        }
                    }
                    // x * 1 -> x, 1 * x -> x
                    BinaryOp::MulInt32 | BinaryOp::MulInt64 |
                    BinaryOp::DivSInt32 | BinaryOp::DivSInt64 |
                    BinaryOp::DivUInt32 | BinaryOp::DivUInt64 => {
                        if is_one(right) {
                            IdentitySide::Right
                        } else if matches!(op, BinaryOp::MulInt32 | BinaryOp::MulInt64) && is_one(left) {
                            IdentitySide::Left
                        } else {
                            IdentitySide::None
                        }
                    }
                    // x & -1 -> x, -1 & x -> x
                    BinaryOp::AndInt32 | BinaryOp::AndInt64 => {
                        if is_all_ones(right) {
                            IdentitySide::Right
                        } else if is_all_ones(left) {
                            IdentitySide::Left
                        } else {
                            IdentitySide::None
                        }
                    }
                    // x == x -> 1, x != x -> 0
            BinaryOp::EqInt32 | BinaryOp::EqInt64 |
                    BinaryOp::EqFloat32 | BinaryOp::EqFloat64 => {
                        if are_expressions_equal(left, right) {
                            if !matches!(op, BinaryOp::EqFloat32 | BinaryOp::EqFloat64) {
                                expr.kind = ExpressionKind::Const(Literal::I32(1));
                                return;
                            }
                        }
                        IdentitySide::None
                    }
                    BinaryOp::NeInt32 | BinaryOp::NeInt64 |
                    BinaryOp::NeFloat32 | BinaryOp::NeFloat64 => {
                        if are_expressions_equal(left, right) {
                            if !matches!(op, BinaryOp::NeFloat32 | BinaryOp::NeFloat64) {
                                expr.kind = ExpressionKind::Const(Literal::I32(0));
                                return;
                            }
                        }
                        IdentitySide::None
                    }
                    _ => IdentitySide::None,
                };

                // Add x op x checks
                let side = if side == IdentitySide::None {
                    if are_expressions_equal(left, right) {
                        match op {
                            BinaryOp::AndInt32
                            | BinaryOp::AndInt64
                            | BinaryOp::OrInt32
                            | BinaryOp::OrInt64 => IdentitySide::Both,
                            BinaryOp::XorInt32 | BinaryOp::XorInt64 => {
                                expr.kind = ExpressionKind::Const(Literal::I32(0));
                                return;
                            }
                            _ => IdentitySide::None,
                        }
                    } else {
                        IdentitySide::None
                    }
                } else {
                    side
                };

                match side {
                    IdentitySide::Right | IdentitySide::Both => {
                        let kind = std::mem::replace(&mut expr.kind, ExpressionKind::Nop);
                        if let ExpressionKind::Binary { mut left, .. } = kind {
                            expr.type_ = left.type_;
                            expr.kind = std::mem::replace(&mut left.kind, ExpressionKind::Nop);
                        }
                    }
                    IdentitySide::Left => {
                        let kind = std::mem::replace(&mut expr.kind, ExpressionKind::Nop);
                        if let ExpressionKind::Binary { mut right, .. } = kind {
                            expr.type_ = right.type_;
                            expr.kind = std::mem::replace(&mut right.kind, ExpressionKind::Nop);
                        }
                    }
                    IdentitySide::None => {}
                }
            }
            ExpressionKind::RefAs { op, value } => {
                if *op == RefAsOp::NonNull && !value.type_.is_nullable() {
                    // Identity
                    let kind = std::mem::replace(&mut expr.kind, ExpressionKind::Nop);
                    if let ExpressionKind::RefAs { mut value, .. } = kind {
                        expr.type_ = value.type_;
                        expr.kind = std::mem::replace(&mut value.kind, ExpressionKind::Nop);
                    }
                }
            }
            ExpressionKind::RefEq { left, right } => {
                if are_expressions_equal(left, right) {
                    expr.kind = ExpressionKind::Const(Literal::I32(1));
                }
            }
            ExpressionKind::RefCast { op, value, type_ } => {
                if *op == RefCastOp::Cast && value.type_ == *type_ {
                    // Constant-time identity: casting to the same type
                    let kind = std::mem::replace(&mut expr.kind, ExpressionKind::Nop);
                    if let ExpressionKind::RefCast { mut value, .. } = kind {
                        expr.type_ = value.type_;
                        expr.kind = std::mem::replace(&mut value.kind, ExpressionKind::Nop);
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::{ExprRef, Expression, ExpressionKind};
    use crate::ir::module::Function;
    use crate::ir::{HeapType, Literal, Type};
    use bumpalo::Bump;

    #[test]
    fn test_simplify_identity_add_zero() {
        let bump = Bump::new();
        let val = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(42)),
            type_: Type::I32,
        }));
        let zero = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(0)),
            type_: Type::I32,
        }));
        let binary = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Binary {
                op: BinaryOp::AddInt32,
                left: val,
                right: zero,
            },
            type_: Type::I32,
        }));

        let func = Function::new("test".into(), Type::NONE, Type::I32, vec![], Some(binary));
        let mut module = Module::new(&bump);
        module.add_function(func);

        SimplifyIdentity.run(&mut module);

        let body = module.functions[0].body.as_ref().unwrap();
        assert!(matches!(body.kind, ExpressionKind::Const(Literal::I32(42))));
    }

    #[test]
    fn test_simplify_identity_ref_as_non_null() {
        let bump = Bump::new();
        let non_null_ref = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(0)),
            type_: Type::new(HeapType::FUNC, false),
        }));

        let ref_as = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::RefAs {
                op: RefAsOp::NonNull,
                value: non_null_ref,
            },
            type_: Type::new(HeapType::FUNC, false),
        }));

        let func = Function::new("test".into(), Type::NONE, Type::I32, vec![], Some(ref_as));
        let mut module = Module::new(&bump);
        module.add_function(func);

        SimplifyIdentity.run(&mut module);

        let body = module.functions[0].body.as_ref().unwrap();
        assert!(matches!(body.kind, ExpressionKind::Const(Literal::I32(0))));
    }

    #[test]
    fn test_simplify_identity_eqz_eqz_eqz() {
        let bump = Bump::new();
        let x = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::LocalGet { index: 0 },
            type_: Type::I32,
        }));

        let eqz1 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Unary {
                op: UnaryOp::EqZInt32,
                value: x,
            },
            type_: Type::I32,
        }));
        let eqz2 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Unary {
                op: UnaryOp::EqZInt32,
                value: eqz1,
            },
            type_: Type::I32,
        }));
        let eqz3 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Unary {
                op: UnaryOp::EqZInt32,
                value: eqz2,
            },
            type_: Type::I32,
        }));

        let func = Function::new("test".into(), Type::NONE, Type::I32, vec![], Some(eqz3));
        let mut module = Module::new(&bump);
        module.add_function(func);

        SimplifyIdentity.run(&mut module);

        let body = module.functions[0].body.as_ref().unwrap();
        if let ExpressionKind::Unary { op, value } = &body.kind {
            assert_eq!(*op, UnaryOp::EqZInt32);
            assert!(matches!(value.kind, ExpressionKind::LocalGet { .. }));
        } else {
            panic!("Expected Unary, got {:?}", body.kind);
        }
    }
}
