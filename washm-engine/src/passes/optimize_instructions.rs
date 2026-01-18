use crate::analysis::patterns::{MatchEnv, Pattern, PatternMatcher, PatternOp, PatternUnaryOp};
use crate::ir::expression::{ExprRef, ExpressionKind, IrBuilder};
use crate::ir::module::Module;
use crate::ir::ops::{BinaryOp, UnaryOp};
use crate::passes::Pass;
use crate::ir::visitor::Visitor;
use crate::ir::{Literal, Type};
use std::ops::Neg;

/// OptimizeInstructions pass: Algebraic simplifications and strength reduction
pub struct OptimizeInstructions {
    matcher: PatternMatcher,
}

impl Default for OptimizeInstructions {
    fn default() -> Self {
        Self::new()
    }
}

impl OptimizeInstructions {
    pub fn new() -> Self {
        let mut matcher = PatternMatcher::new();
        Self::register_sign_extension_identities(&mut matcher);
        Self::register_constant_folding(&mut matcher);
        Self::register_algebraic_identities(&mut matcher);
        Self::register_reassociation(&mut matcher);
        Self::register_strength_reduction(&mut matcher);
        Self::register_comparison_optimizations(&mut matcher);
        Self::register_bitwise_optimizations(&mut matcher);
        Self::register_fp_optimizations(&mut matcher);
        Self::register_select_optimizations(&mut matcher);
        Self { matcher }
    }

    fn register_sign_extension_identities(matcher: &mut PatternMatcher) {
        // --- Redundant Sign Extensions ---

        // extend_s8(extend_s8(x)) -> extend_s8(x)
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::ExtendS8Int32,
                Pattern::unary(UnaryOp::ExtendS8Int32, Pattern::Var("x")),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::ExtendS8Int32, *x, Type::I32))
            },
        );

        matcher.add_rule(
            Pattern::unary(
                UnaryOp::ExtendS16Int32,
                Pattern::unary(UnaryOp::ExtendS16Int32, Pattern::Var("x")),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::ExtendS16Int32, *x, Type::I32))
            },
        );

        matcher.add_rule(
            Pattern::unary(
                UnaryOp::ExtendS8Int64,
                Pattern::unary(UnaryOp::ExtendS8Int64, Pattern::Var("x")),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::ExtendS8Int64, *x, Type::I64))
            },
        );

        matcher.add_rule(
            Pattern::unary(
                UnaryOp::ExtendS16Int64,
                Pattern::unary(UnaryOp::ExtendS16Int64, Pattern::Var("x")),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::ExtendS16Int64, *x, Type::I64))
            },
        );

        matcher.add_rule(
            Pattern::unary(
                UnaryOp::ExtendS32Int64,
                Pattern::unary(UnaryOp::ExtendS32Int64, Pattern::Var("x")),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::ExtendS32Int64, *x, Type::I64))
            },
        );

        // --- Cross-Bit-Width Sign Extension Identities ---

        // extend_s16(extend_s8(x)) -> extend_s8(x)
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::ExtendS16Int32,
                Pattern::unary(UnaryOp::ExtendS8Int32, Pattern::Var("x")),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::ExtendS8Int32, *x, Type::I32))
            },
        );

        // extend_s8(extend_s16(x)) -> extend_s8(x)
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::ExtendS8Int32,
                Pattern::unary(UnaryOp::ExtendS16Int32, Pattern::Var("x")),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::ExtendS8Int32, *x, Type::I32))
            },
        );

        // i64.extend_s32(i64.extend_s16(x)) -> i64.extend_s16(x)
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::ExtendS32Int64,
                Pattern::unary(UnaryOp::ExtendS16Int64, Pattern::Var("x")),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::ExtendS16Int64, *x, Type::I64))
            },
        );

        // i64.extend_s32(i64.extend_s8(x)) -> i64.extend_s8(x)
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::ExtendS32Int64,
                Pattern::unary(UnaryOp::ExtendS8Int64, Pattern::Var("x")),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::ExtendS8Int64, *x, Type::I64))
            },
        );

        // i64.extend_s16(i64.extend_s8(x)) -> i64.extend_s8(x)
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::ExtendS16Int64,
                Pattern::unary(UnaryOp::ExtendS8Int64, Pattern::Var("x")),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::ExtendS8Int64, *x, Type::I64))
            },
        );

        // i64.extend_s8(i64.extend_s32(x)) -> i64.extend_s8(x)
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::ExtendS8Int64,
                Pattern::unary(UnaryOp::ExtendS32Int64, Pattern::Var("x")),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::ExtendS8Int64, *x, Type::I64))
            },
        );

        // --- Shift to Sign Extension Folding ---

        // (x << 24) >>s 24 -> extend_s8(x)
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::ShrSInt32,
                Pattern::binary(
                    BinaryOp::ShlInt32,
                    Pattern::Var("x"),
                    Pattern::Const(Literal::I32(24)),
                ),
                Pattern::Const(Literal::I32(24)),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::ExtendS8Int32, *x, Type::I32))
            },
        );

        // (x << 16) >>s 16 -> extend_s16(x)
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::ShrSInt32,
                Pattern::binary(
                    BinaryOp::ShlInt32,
                    Pattern::Var("x"),
                    Pattern::Const(Literal::I32(16)),
                ),
                Pattern::Const(Literal::I32(16)),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::ExtendS16Int32, *x, Type::I32))
            },
        );

        // (x << 56) >>s 56 -> extend_s8(x) (i64)
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::ShrSInt64,
                Pattern::binary(
                    BinaryOp::ShlInt64,
                    Pattern::Var("x"),
                    Pattern::Const(Literal::I64(56)),
                ),
                Pattern::Const(Literal::I64(56)),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::ExtendS8Int64, *x, Type::I64))
            },
        );

        // (x << 48) >>s 48 -> extend_s16(x) (i64)
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::ShrSInt64,
                Pattern::binary(
                    BinaryOp::ShlInt64,
                    Pattern::Var("x"),
                    Pattern::Const(Literal::I64(48)),
                ),
                Pattern::Const(Literal::I64(48)),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::ExtendS16Int64, *x, Type::I64))
            },
        );

        // (x << 32) >>s 32 -> extend_s32(x) (i64)
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::ShrSInt64,
                Pattern::binary(
                    BinaryOp::ShlInt64,
                    Pattern::Var("x"),
                    Pattern::Const(Literal::I64(32)),
                ),
                Pattern::Const(Literal::I64(32)),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::ExtendS32Int64, *x, Type::I64))
            },
        );
    }

    fn register_select_optimizations(matcher: &mut PatternMatcher) {
        // select(condition, x, x) -> x
        matcher.add_rule(
            Pattern::select(Pattern::Any, Pattern::Var("x"), Pattern::Var("x")),
            |env, _| env.get("x").copied(),
        );

        // select(1, x, y) -> x
        matcher.add_rule(
            Pattern::select(
                Pattern::Const(Literal::I32(1)),
                Pattern::Var("x"),
                Pattern::Var("y"),
            ),
            |env, _| env.get("x").copied(),
        );

        // select(0, x, y) -> y
        matcher.add_rule(
            Pattern::select(
                Pattern::Const(Literal::I32(0)),
                Pattern::Var("x"),
                Pattern::Var("y"),
            ),
            |env, _| env.get("y").copied(),
        );
    }

    fn register_fp_optimizations(matcher: &mut PatternMatcher) {
        use crate::ir::ops::UnaryOp;

        // --- Double Negation / Abs ---

        // f32.neg(f32.neg(x)) -> x
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::NegFloat32,
                Pattern::unary(UnaryOp::NegFloat32, Pattern::Var("x")),
            ),
            |env, _| env.get("x").copied(),
        );

        // f64.neg(f64.neg(x)) -> x
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::NegFloat64,
                Pattern::unary(UnaryOp::NegFloat64, Pattern::Var("x")),
            ),
            |env, _| env.get("x").copied(),
        );

        // f32.abs(f32.abs(x)) -> f32.abs(x)
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::AbsFloat32,
                Pattern::unary(UnaryOp::AbsFloat32, Pattern::Var("x")),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::AbsFloat32, *x, Type::F32))
            },
        );

        // f64.abs(f64.abs(x)) -> f64.abs(x)
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::AbsFloat64,
                Pattern::unary(UnaryOp::AbsFloat64, Pattern::Var("x")),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::AbsFloat64, *x, Type::F64))
            },
        );

        // --- Negation Flipping (Multiplication) ---

        // f32.neg(x) * f32.neg(y) -> x * y
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::MulFloat32,
                Pattern::unary(UnaryOp::NegFloat32, Pattern::Var("x")),
                Pattern::unary(UnaryOp::NegFloat32, Pattern::Var("y")),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let y = env.get("y")?;
                let builder = IrBuilder::new(bump);
                Some(builder.binary(BinaryOp::MulFloat32, *x, *y, Type::F32))
            },
        );

        // f64.neg(x) * f64.neg(y) -> x * y
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::MulFloat64,
                Pattern::unary(UnaryOp::NegFloat64, Pattern::Var("x")),
                Pattern::unary(UnaryOp::NegFloat64, Pattern::Var("y")),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let y = env.get("y")?;
                let builder = IrBuilder::new(bump);
                Some(builder.binary(BinaryOp::MulFloat64, *x, *y, Type::F64))
            },
        );

        // --- Negation Flipping (Division) ---

        // f32.neg(x) / f32.neg(y) -> x / y
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::DivFloat32,
                Pattern::unary(UnaryOp::NegFloat32, Pattern::Var("x")),
                Pattern::unary(UnaryOp::NegFloat32, Pattern::Var("y")),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let y = env.get("y")?;
                let builder = IrBuilder::new(bump);
                Some(builder.binary(BinaryOp::DivFloat32, *x, *y, Type::F32))
            },
        );

        // f64.neg(x) / f64.neg(y) -> x / y
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::DivFloat64,
                Pattern::unary(UnaryOp::NegFloat64, Pattern::Var("x")),
                Pattern::unary(UnaryOp::NegFloat64, Pattern::Var("y")),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let y = env.get("y")?;
                let builder = IrBuilder::new(bump);
                Some(builder.binary(BinaryOp::DivFloat64, *x, *y, Type::F64))
            },
        );

        // --- Copysign ---

        // f32.copysign(x, x) -> x
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::CopySignFloat32,
                Pattern::Var("x"),
                Pattern::Var("x"),
            ),
            |env, _| env.get("x").copied(),
        );

        // f64.copysign(x, x) -> x
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::CopySignFloat64,
                Pattern::Var("x"),
                Pattern::Var("x"),
            ),
            |env, _| env.get("x").copied(),
        );

        // --- Min/Max ---

        // f32.min(x, x) -> x
        matcher.add_rule(
            Pattern::binary(BinaryOp::MinFloat32, Pattern::Var("x"), Pattern::Var("x")),
            |env, _| env.get("x").copied(),
        );

        // f32.max(x, x) -> x
        matcher.add_rule(
            Pattern::binary(BinaryOp::MaxFloat32, Pattern::Var("x"), Pattern::Var("x")),
            |env, _| env.get("x").copied(),
        );

        // f64.min(x, x) -> x
        matcher.add_rule(
            Pattern::binary(BinaryOp::MinFloat64, Pattern::Var("x"), Pattern::Var("x")),
            |env, _| env.get("x").copied(),
        );

        // f64.max(x, x) -> x
        matcher.add_rule(
            Pattern::binary(BinaryOp::MaxFloat64, Pattern::Var("x"), Pattern::Var("x")),
            |env, _| env.get("x").copied(),
        );
    }

    fn register_bitwise_optimizations(matcher: &mut PatternMatcher) {
        // --- Shift Truncation ---

        // (x << C) where C >= 32 -> x << (C % 32)
        matcher.add_rule(
            Pattern::binary(BinaryOp::ShlInt32, Pattern::Var("x"), Pattern::AnyConst),
            |env, bump| {
                let x = env.get("x")?;
                let c = env.get_const("right")?.get_i32();
                let shift = (c as u32 % 32) as i32;
                if c as u32 >= 32 {
                    let builder = IrBuilder::new(bump);
                    return Some(builder.binary(
                        BinaryOp::ShlInt32,
                        *x,
                        builder.const_(Literal::I32(shift)),
                        Type::I32,
                    ));
                }
                None
            },
        );

        // (x >>u C) where C >= 32 -> x >>u (C % 32)
        matcher.add_rule(
            Pattern::binary(BinaryOp::ShrUInt32, Pattern::Var("x"), Pattern::AnyConst),
            |env, bump| {
                let x = env.get("x")?;
                let c = env.get_const("right")?.get_i32();
                let shift = (c as u32 % 32) as i32;
                if c as u32 >= 32 {
                    let builder = IrBuilder::new(bump);
                    return Some(builder.binary(
                        BinaryOp::ShrUInt32,
                        *x,
                        builder.const_(Literal::I32(shift)),
                        Type::I32,
                    ));
                }
                None
            },
        );

        // (x >>s C) where C >= 32 -> x >>s (C % 32)
        matcher.add_rule(
            Pattern::binary(BinaryOp::ShrSInt32, Pattern::Var("x"), Pattern::AnyConst),
            |env, bump| {
                let x = env.get("x")?;
                let c = env.get_const("right")?.get_i32();
                let shift = (c as u32 % 32) as i32;
                if c as u32 >= 32 {
                    let builder = IrBuilder::new(bump);
                    return Some(builder.binary(
                        BinaryOp::ShrSInt32,
                        *x,
                        builder.const_(Literal::I32(shift)),
                        Type::I32,
                    ));
                }
                None
            },
        );

        // x << 0 -> x
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::ShlInt32,
                Pattern::Var("x"),
                Pattern::Const(Literal::I32(0)),
            ),
            |env, _| env.get("x").copied(),
        );

        // x >>u 0 -> x
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::ShrUInt32,
                Pattern::Var("x"),
                Pattern::Const(Literal::I32(0)),
            ),
            |env, _| env.get("x").copied(),
        );

        // x >>s 0 -> x
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::ShrSInt32,
                Pattern::Var("x"),
                Pattern::Const(Literal::I32(0)),
            ),
            |env, _| env.get("x").copied(),
        );

        // I64 Shift Truncation

        // (x << C) where C >= 64 -> x << (C % 64)
        matcher.add_rule(
            Pattern::binary(BinaryOp::ShlInt64, Pattern::Var("x"), Pattern::AnyConst),
            |env, bump| {
                let x = env.get("x")?;
                let c = env.get_const("right")?.get_i64();
                if c as u64 >= 64 {
                    let builder = IrBuilder::new(bump);
                    return Some(builder.binary(
                        BinaryOp::ShlInt64,
                        *x,
                        builder.const_(Literal::I64((c as u64 & 63) as i64)),
                        Type::I64,
                    ));
                }
                None
            },
        );

        // (x >>u C) where C >= 64 -> x >>u (C % 64)
        matcher.add_rule(
            Pattern::binary(BinaryOp::ShrUInt64, Pattern::Var("x"), Pattern::AnyConst),
            |env, bump| {
                let x = env.get("x")?;
                let c = env.get_const("right")?.get_i64();
                if c as u64 >= 64 {
                    let builder = IrBuilder::new(bump);
                    return Some(builder.binary(
                        BinaryOp::ShrUInt64,
                        *x,
                        builder.const_(Literal::I64((c as u64 & 63) as i64)),
                        Type::I64,
                    ));
                }
                None
            },
        );

        // (x >>s C) where C >= 64 -> x >>s (C % 64)
        matcher.add_rule(
            Pattern::binary(BinaryOp::ShrSInt64, Pattern::Var("x"), Pattern::AnyConst),
            |env, bump| {
                let x = env.get("x")?;
                let c = env.get_const("right")?.get_i64();
                if c as u64 >= 64 {
                    let builder = IrBuilder::new(bump);
                    return Some(builder.binary(
                        BinaryOp::ShrSInt64,
                        *x,
                        builder.const_(Literal::I64((c as u64 & 63) as i64)),
                        Type::I64,
                    ));
                }
                None
            },
        );

        // --- Bitwise Identities ---

        // x & (x ^ -1) -> 0 (x & ~x)
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::AndInt32,
                Pattern::Var("x"),
                Pattern::binary(
                    BinaryOp::XorInt32,
                    Pattern::Var("x"),
                    Pattern::Const(Literal::I32(-1)),
                ),
            ),
            |_env, bump| {
                let builder = IrBuilder::new(bump);
                Some(builder.const_(Literal::I32(0)))
            },
        );

        // x | (x ^ -1) -> -1 (x | ~x)
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::OrInt32,
                Pattern::Var("x"),
                Pattern::binary(
                    BinaryOp::XorInt32,
                    Pattern::Var("x"),
                    Pattern::Const(Literal::I32(-1)),
                ),
            ),
            |_env, bump| {
                let builder = IrBuilder::new(bump);
                Some(builder.const_(Literal::I32(-1)))
            },
        );

        // eqz(x | y) -> eqz(x) & eqz(y)
        // This is only helpful if eqz(x) and eqz(y) can be folded or further simplified.
        // Actually, sometimes (x | y) == 0 is better as eqz(x) & eqz(y) if we can then fold one side.
        // But in general, (x | y) == 0 is one op, while eqz(x) & eqz(y) is three.
        // C++ implementation does:
        // if (curr->op == EqZInt32) { ... if (inner->op == OrInt32) ... }
        // Let's stick to what C++ does if possible.

        // --- De Morgan's ---

        // (x ^ -1) & (y ^ -1) -> (x | y) ^ -1
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::AndInt32,
                Pattern::binary(
                    BinaryOp::XorInt32,
                    Pattern::Var("x"),
                    Pattern::Const(Literal::I32(-1)),
                ),
                Pattern::binary(
                    BinaryOp::XorInt32,
                    Pattern::Var("y"),
                    Pattern::Const(Literal::I32(-1)),
                ),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let y = env.get("y")?;
                let builder = IrBuilder::new(bump);
                let or = builder.binary(BinaryOp::OrInt32, *x, *y, Type::I32);
                Some(builder.binary(
                    BinaryOp::XorInt32,
                    or,
                    builder.const_(Literal::I32(-1)),
                    Type::I32,
                ))
            },
        );

        // (x ^ -1) | (y ^ -1) -> (x & y) ^ -1
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::OrInt32,
                Pattern::binary(
                    BinaryOp::XorInt32,
                    Pattern::Var("x"),
                    Pattern::Const(Literal::I32(-1)),
                ),
                Pattern::binary(
                    BinaryOp::XorInt32,
                    Pattern::Var("y"),
                    Pattern::Const(Literal::I32(-1)),
                ),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let y = env.get("y")?;
                let builder = IrBuilder::new(bump);
                let and = builder.binary(BinaryOp::AndInt32, *x, *y, Type::I32);
                Some(builder.binary(
                    BinaryOp::XorInt32,
                    and,
                    builder.const_(Literal::I32(-1)),
                    Type::I32,
                ))
            },
        );

        // --- Absorption Laws ---

        // x & (x | y) -> x
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::AndInt32,
                Pattern::Var("x"),
                Pattern::binary(BinaryOp::OrInt32, Pattern::Var("x"), Pattern::Var("y")),
            ),
            |env, _| env.get("x").copied(),
        );

        // (x | y) & x -> x
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::AndInt32,
                Pattern::binary(BinaryOp::OrInt32, Pattern::Var("x"), Pattern::Var("y")),
                Pattern::Var("x"),
            ),
            |env, _| env.get("x").copied(),
        );

        // x | (x & y) -> x
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::OrInt32,
                Pattern::Var("x"),
                Pattern::binary(BinaryOp::AndInt32, Pattern::Var("x"), Pattern::Var("y")),
            ),
            |env, _| env.get("x").copied(),
        );

        // (x & y) | x -> x
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::OrInt32,
                Pattern::binary(BinaryOp::AndInt32, Pattern::Var("x"), Pattern::Var("y")),
                Pattern::Var("x"),
            ),
            |env, _| env.get("x").copied(),
        );

        // --- Xor Identities ---

        // (x ^ y) ^ y -> x
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::XorInt32,
                Pattern::binary(BinaryOp::XorInt32, Pattern::Var("x"), Pattern::Var("y")),
                Pattern::Var("y"),
            ),
            |env, _| env.get("x").copied(),
        );

        // y ^ (x ^ y) -> x
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::XorInt32,
                Pattern::Var("y"),
                Pattern::binary(BinaryOp::XorInt32, Pattern::Var("x"), Pattern::Var("y")),
            ),
            |env, _| env.get("x").copied(),
        );

        // --- Shift-Mask Interactions ---

        // (x << C) & M where (M & ((1 << C) - 1)) == 0
        // If we shift left by C, the lowest C bits are 0. If the mask also has those bits as 0, the mask is redundant for those bits.
        // Wait, if (M & ((1 << C) - 1)) == 0 it means M doesn't care about the bits that became 0.
        // But (x << C) already has those bits as 0. So the mask doesn't change anything in those bits.
        // However, if M has other bits as 0, it might still be useful.
        // If M only has bits set that are >= C, then M might be simplifying the value of x before shift.
        // (x << C) & (Mask << C) -> (x & Mask) << C

        matcher.add_rule(
            Pattern::binary(
                BinaryOp::AndInt32,
                Pattern::binary(BinaryOp::ShlInt32, Pattern::Var("x"), Pattern::AnyConst),
                Pattern::AnyConst,
            ),
            |env, bump| {
                let x = env.get("x")?;
                let c = env.get_const("left")?.get_i32() as u32 & 31;
                let m = env.get_const("right")?.get_i32();

                if c > 0 {
                    let mask_shifted = m >> c;
                    // Check if m has no bits set in the lower c bits
                    let low_bits_mask = (1i32 << c).wrapping_sub(1);
                    if (m & low_bits_mask) == 0 {
                        // (x << c) & m  =>  (x & (m >> c)) << c
                        let builder = IrBuilder::new(bump);
                        let inner_and = builder.binary(
                            BinaryOp::AndInt32,
                            *x,
                            builder.const_(Literal::I32(mask_shifted)),
                            Type::I32,
                        );
                        return Some(builder.binary(
                            BinaryOp::ShlInt32,
                            inner_and,
                            builder.const_(Literal::I32(c as i32)),
                            Type::I32,
                        ));
                    }
                }
                None
            },
        );
    }

    fn register_constant_folding(matcher: &mut PatternMatcher) {
        // Fold binary operations on constants.
        matcher.add_rule(
            Pattern::binary(PatternOp::AnyOp, Pattern::AnyConst, Pattern::AnyConst),
            |env, bump| {
                let left = env.get_const("left")?;
                let right = env.get_const("right")?;
                let op = env.get_op()?;

                let builder = IrBuilder::new(bump);

                // It is important to check for the operation because
                // not all binary operations can be folded.
                if let Some(val) = Self::eval_binary_op(op, left, right) {
                    Some(builder.const_(val))
                } else {
                    None
                }
            },
        );

        // Fold unary operations on constants.
        matcher.add_rule(
            Pattern::unary(PatternUnaryOp::AnyOp, Pattern::AnyConst),
            |env, bump| {
                let value = env.get_const("value")?;
                let op = env.get_unary_op()?;

                let builder = IrBuilder::new(bump);

                if let Some(val) = Self::eval_unary_op(op, value) {
                    Some(builder.const_(val))
                } else {
                    None
                }
            },
        );
    }

    fn eval_binary_op(op: BinaryOp, left: Literal, right: Literal) -> Option<Literal> {
        use BinaryOp::*;
        match op {
            AddInt32 => Some(Literal::I32(left.get_i32().wrapping_add(right.get_i32()))),
            SubInt32 => Some(Literal::I32(left.get_i32().wrapping_sub(right.get_i32()))),
            MulInt32 => Some(Literal::I32(left.get_i32().wrapping_mul(right.get_i32()))),
            DivSInt32 => {
                let r = right.get_i32();
                if r == 0 {
                    None // division by zero
                } else {
                    Some(Literal::I32(left.get_i32().wrapping_div(r)))
                }
            }
            DivUInt32 => {
                let r = right.get_u32();
                if r == 0 {
                    None // division by zero
                } else {
                    Some(Literal::I32((left.get_u32() / r) as i32))
                }
            }
            RemSInt32 => {
                let r = right.get_i32();
                if r == 0 {
                    None // division by zero
                } else {
                    Some(Literal::I32(left.get_i32().wrapping_rem(r)))
                }
            }
            RemUInt32 => {
                let r = right.get_u32();
                if r == 0 {
                    None // division by zero
                } else {
                    Some(Literal::I32((left.get_u32() % r) as i32))
                }
            }
            AndInt32 => Some(Literal::I32(left.get_i32() & right.get_i32())),
            OrInt32 => Some(Literal::I32(left.get_i32() | right.get_i32())),
            XorInt32 => Some(Literal::I32(left.get_i32() ^ right.get_i32())),
            ShlInt32 => Some(Literal::I32(left.get_i32().wrapping_shl(right.get_u32()))),
            ShrUInt32 => Some(Literal::I32(
                left.get_u32().wrapping_shr(right.get_u32()) as i32
            )),
            ShrSInt32 => Some(Literal::I32(left.get_i32().wrapping_shr(right.get_u32()))),
            RotLInt32 => Some(Literal::I32(left.get_i32().rotate_left(right.get_u32()))),
            RotRInt32 => Some(Literal::I32(left.get_i32().rotate_right(right.get_u32()))),

            EqInt32 => Some(Literal::I32((left.get_i32() == right.get_i32()) as i32)),
            NeInt32 => Some(Literal::I32((left.get_i32() != right.get_i32()) as i32)),
            LtSInt32 => Some(Literal::I32((left.get_i32() < right.get_i32()) as i32)),
            LtUInt32 => Some(Literal::I32((left.get_u32() < right.get_u32()) as i32)),
            LeSInt32 => Some(Literal::I32((left.get_i32() <= right.get_i32()) as i32)),
            LeUInt32 => Some(Literal::I32((left.get_u32() <= right.get_u32()) as i32)),
            GtSInt32 => Some(Literal::I32((left.get_i32() > right.get_i32()) as i32)),
            GtUInt32 => Some(Literal::I32((left.get_u32() > right.get_u32()) as i32)),
            GeSInt32 => Some(Literal::I32((left.get_i32() >= right.get_i32()) as i32)),
            GeUInt32 => Some(Literal::I32((left.get_u32() >= right.get_u32()) as i32)),

            AddInt64 => Some(Literal::I64(left.get_i64().wrapping_add(right.get_i64()))),
            SubInt64 => Some(Literal::I64(left.get_i64().wrapping_sub(right.get_i64()))),
            MulInt64 => Some(Literal::I64(left.get_i64().wrapping_mul(right.get_i64()))),
            DivSInt64 => {
                let r = right.get_i64();
                if r == 0 {
                    None // division by zero
                } else {
                    Some(Literal::I64(left.get_i64().wrapping_div(r)))
                }
            }
            DivUInt64 => {
                let r = right.get_u64();
                if r == 0 {
                    None // division by zero
                } else {
                    Some(Literal::I64((left.get_u64() / r) as i64))
                }
            }
            RemSInt64 => {
                let r = right.get_i64();
                if r == 0 {
                    None // division by zero
                } else {
                    Some(Literal::I64(left.get_i64().wrapping_rem(r)))
                }
            }
            RemUInt64 => {
                let r = right.get_u64();
                if r == 0 {
                    None // division by zero
                } else {
                    Some(Literal::I64((left.get_u64() % r) as i64))
                }
            }
            AndInt64 => Some(Literal::I64(left.get_i64() & right.get_i64())),
            OrInt64 => Some(Literal::I64(left.get_i64() | right.get_i64())),
            XorInt64 => Some(Literal::I64(left.get_i64() ^ right.get_i64())),
            ShlInt64 => Some(Literal::I64(left.get_i64().wrapping_shl(right.get_u32()))),
            ShrUInt64 => Some(Literal::I64(
                left.get_u64().wrapping_shr(right.get_u32()) as i64
            )),
            ShrSInt64 => Some(Literal::I64(left.get_i64().wrapping_shr(right.get_u32()))),
            RotLInt64 => Some(Literal::I64(left.get_i64().rotate_left(right.get_u32()))),
            RotRInt64 => Some(Literal::I64(left.get_i64().rotate_right(right.get_u32()))),

            EqInt64 => Some(Literal::I32((left.get_i64() == right.get_i64()) as i32)),
            NeInt64 => Some(Literal::I32((left.get_i64() != right.get_i64()) as i32)),
            LtSInt64 => Some(Literal::I32((left.get_i64() < right.get_i64()) as i32)),
            LtUInt64 => Some(Literal::I32((left.get_u64() < right.get_u64()) as i32)),
            LeSInt64 => Some(Literal::I32((left.get_i64() <= right.get_i64()) as i32)),
            LeUInt64 => Some(Literal::I32((left.get_u64() <= right.get_u64()) as i32)),
            GtSInt64 => Some(Literal::I32((left.get_i64() > right.get_i64()) as i32)),
            GtUInt64 => Some(Literal::I32((left.get_u64() > right.get_u64()) as i32)),
            GeSInt64 => Some(Literal::I32((left.get_i64() >= right.get_i64()) as i32)),
            GeUInt64 => Some(Literal::I32((left.get_u64() >= right.get_u64()) as i32)),

            AddFloat32 => Some(Literal::F32(left.get_f32() + right.get_f32())),
            SubFloat32 => Some(Literal::F32(left.get_f32() - right.get_f32())),
            MulFloat32 => Some(Literal::F32(left.get_f32() * right.get_f32())),
            DivFloat32 => {
                let left_val = left.get_f32();
                let right_val = right.get_f32();
                let res = left_val / right_val;

                // Return None if the result is NaN or Infinite, as the tests expect these to not be folded.
                if res.is_nan() || res.is_infinite() {
                    None
                } else {
                    Some(Literal::F32(res))
                }
            }
            MinFloat32 => Some(Literal::F32(left.get_f32().min(right.get_f32()))),
            MaxFloat32 => Some(Literal::F32(left.get_f32().max(right.get_f32()))),
            CopySignFloat32 => Some(Literal::F32(left.get_f32().copysign(right.get_f32()))),

            EqFloat32 => Some(Literal::I32((left.get_f32() == right.get_f32()) as i32)),
            NeFloat32 => Some(Literal::I32((left.get_f32() != right.get_f32()) as i32)),
            LtFloat32 => Some(Literal::I32((left.get_f32() < right.get_f32()) as i32)),
            LeFloat32 => Some(Literal::I32((left.get_f32() <= right.get_f32()) as i32)),
            GtFloat32 => Some(Literal::I32((left.get_f32() > right.get_f32()) as i32)),
            GeFloat32 => Some(Literal::I32((left.get_f32() >= right.get_f32()) as i32)),

            AddFloat64 => Some(Literal::F64(left.get_f64() + right.get_f64())),
            SubFloat64 => Some(Literal::F64(left.get_f64() - right.get_f64())),
            MulFloat64 => Some(Literal::F64(left.get_f64() * right.get_f64())),
            DivFloat64 => {
                let left_val = left.get_f64();
                let right_val = right.get_f64();
                let res = left_val / right_val;

                // Return None if the result is NaN or Infinite, as the tests expect these to not be folded.
                if res.is_nan() || res.is_infinite() {
                    None
                } else {
                    Some(Literal::F64(res))
                }
            }
            MinFloat64 => Some(Literal::F64(left.get_f64().min(right.get_f64()))),
            MaxFloat64 => Some(Literal::F64(left.get_f64().max(right.get_f64()))),
            CopySignFloat64 => Some(Literal::F64(left.get_f64().copysign(right.get_f64()))),

            EqFloat64 => Some(Literal::I32((left.get_f64() == right.get_f64()) as i32)),
            NeFloat64 => Some(Literal::I32((left.get_f64() != right.get_f64()) as i32)),
            LtFloat64 => Some(Literal::I32((left.get_f64() < right.get_f64()) as i32)),
            LeFloat64 => Some(Literal::I32((left.get_f64() <= right.get_f64()) as i32)),
            GtFloat64 => Some(Literal::I32((left.get_f64() > right.get_f64()) as i32)),
            GeFloat64 => Some(Literal::I32((left.get_f64() >= right.get_f64()) as i32)),
        }
    }

    fn eval_unary_op(op: UnaryOp, value: Literal) -> Option<Literal> {
        use UnaryOp::*;
        match op {
            NegFloat32 => {
                let res = -value.get_f32();
                if res.is_nan() {
                    None
                } else {
                    Some(Literal::F32(res))
                }
            }
            AbsFloat32 => {
                let res = value.get_f32().abs();
                if res.is_nan() {
                    None
                } else {
                    Some(Literal::F32(res))
                }
            }
            CeilFloat32 => {
                let res = value.get_f32().ceil();
                if res.is_nan() {
                    None
                } else {
                    Some(Literal::F32(res))
                }
            }
            FloorFloat32 => {
                let res = value.get_f32().floor();
                if res.is_nan() {
                    None
                } else {
                    Some(Literal::F32(res))
                }
            }
            TruncFloat32 => {
                let res = value.get_f32().trunc();
                if res.is_nan() {
                    None
                } else {
                    Some(Literal::F32(res))
                }
            }
            NearestFloat32 => {
                let res = value.get_f32().round();
                if res.is_nan() {
                    None
                } else {
                    Some(Literal::F32(res))
                }
            }
            SqrtFloat32 => {
                let res = value.get_f32().sqrt();
                if res.is_nan() {
                    None
                } else {
                    Some(Literal::F32(res))
                }
            }

            NegFloat64 => {
                let res = -value.get_f64();
                if res.is_nan() {
                    None
                } else {
                    Some(Literal::F64(res))
                }
            }
            AbsFloat64 => {
                let res = value.get_f64().abs();
                if res.is_nan() {
                    None
                } else {
                    Some(Literal::F64(res))
                }
            }
            CeilFloat64 => {
                let res = value.get_f64().ceil();
                if res.is_nan() {
                    None
                } else {
                    Some(Literal::F64(res))
                }
            }
            FloorFloat64 => {
                let res = value.get_f64().floor();
                if res.is_nan() {
                    None
                } else {
                    Some(Literal::F64(res))
                }
            }
            TruncFloat64 => {
                let res = value.get_f64().trunc();
                if res.is_nan() {
                    None
                } else {
                    Some(Literal::F64(res))
                }
            }
            NearestFloat64 => {
                let res = value.get_f64().round();
                if res.is_nan() {
                    None
                } else {
                    Some(Literal::F64(res))
                }
            }
            SqrtFloat64 => {
                let res = value.get_f64().sqrt();
                if res.is_nan() {
                    None
                } else {
                    Some(Literal::F64(res))
                }
            }

            EqZInt32 => Some(Literal::I32((value.get_i32() == 0) as i32)),
            EqZInt64 => Some(Literal::I32((value.get_i64() == 0) as i32)),

            ClzInt32 => Some(Literal::I32(value.get_u32().leading_zeros() as i32)),
            CtzInt32 => Some(Literal::I32(value.get_u32().trailing_zeros() as i32)),
            PopcntInt32 => Some(Literal::I32(value.get_u32().count_ones() as i32)),
            ClzInt64 => Some(Literal::I64(value.get_u64().leading_zeros() as i64)),
            CtzInt64 => Some(Literal::I64(value.get_u64().trailing_zeros() as i64)),
            PopcntInt64 => Some(Literal::I64(value.get_u64().count_ones() as i64)),

            ExtendSInt32 => Some(Literal::I64(value.get_i32() as i64)),
            ExtendUInt32 => Some(Literal::I64(value.get_u32() as i64)),
            WrapInt64 => Some(Literal::I32(value.get_i64() as i32)),

            ExtendS8Int32 => Some(Literal::I32(value.get_i32() as i8 as i32)),
            ExtendS16Int32 => Some(Literal::I32(value.get_i32() as i16 as i32)),
            ExtendS8Int64 => Some(Literal::I64(value.get_i64() as i8 as i64)),
            ExtendS16Int64 => Some(Literal::I64(value.get_i64() as i16 as i64)),
            ExtendS32Int64 => Some(Literal::I64(value.get_i64() as i32 as i64)),

            ConvertSInt32ToFloat32 => Some(Literal::F32(value.get_i32() as f32)),
            ConvertUInt32ToFloat32 => Some(Literal::F32(value.get_u32() as f32)),
            ConvertSInt64ToFloat32 => Some(Literal::F32(value.get_i64() as f32)),
            ConvertUInt64ToFloat32 => Some(Literal::F32(value.get_u64() as f32)),
            ConvertSInt32ToFloat64 => Some(Literal::F64(value.get_i32() as f64)),
            ConvertUInt32ToFloat64 => Some(Literal::F64(value.get_u32() as f64)),
            ConvertSInt64ToFloat64 => Some(Literal::F64(value.get_i64() as f64)),
            ConvertUInt64ToFloat64 => Some(Literal::F64(value.get_u64() as f64)),

            TruncSFloat32ToInt32 => Some(Literal::I32(value.get_f32() as i32)),
            TruncUFloat32ToInt32 => Some(Literal::I32(value.get_f32() as u32 as i32)),
            TruncSFloat32ToInt64 => Some(Literal::I64(value.get_f32() as i64)),
            TruncUFloat32ToInt64 => Some(Literal::I64(value.get_f32() as u64 as i64)),
            TruncSFloat64ToInt32 => Some(Literal::I32(value.get_f64() as i32)),
            TruncUFloat64ToInt32 => Some(Literal::I32(value.get_f64() as u32 as i32)),
            TruncSFloat64ToInt64 => Some(Literal::I64(value.get_f64() as i64)),
            TruncUFloat64ToInt64 => Some(Literal::I64(value.get_f64() as u64 as i64)),

            ReinterpretFloat32 => Some(Literal::I32(value.get_f32().to_bits() as i32)),
            ReinterpretFloat64 => Some(Literal::I64(value.get_f64().to_bits() as i64)),
            ReinterpretInt32 => Some(Literal::F32(f32::from_bits(value.get_u32()))),
            ReinterpretInt64 => Some(Literal::F64(f64::from_bits(value.get_u64()))),
            _ => None,
        }
    }

    fn register_comparison_optimizations(matcher: &mut PatternMatcher) {
        use crate::ir::ops::UnaryOp;

        // x == 0 -> eqz(x)
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::EqInt32,
                Pattern::Var("x"),
                Pattern::Const(Literal::I32(0)),
            ),
            |env: &MatchEnv, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::EqZInt32, *x, Type::I32))
            },
        );

        // 0 == x -> eqz(x)
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::EqInt32,
                Pattern::Const(Literal::I32(0)),
                Pattern::Var("x"),
            ),
            |env: &MatchEnv, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::EqZInt32, *x, Type::I32))
            },
        );

        // x >u 0 -> x != 0
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::GtUInt32,
                Pattern::Var("x"),
                Pattern::Const(Literal::I32(0)),
            ),
            |env: &MatchEnv, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                let zero = builder.const_(Literal::I32(0));
                Some(builder.binary(BinaryOp::NeInt32, *x, zero, Type::I32))
            },
        );

        // x <=u 0 -> x == 0 -> eqz(x)
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::LeUInt32,
                Pattern::Var("x"),
                Pattern::Const(Literal::I32(0)),
            ),
            |env: &MatchEnv, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::EqZInt32, *x, Type::I32))
            },
        );

        // x <u 1 -> x == 0 -> eqz(x)
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::LtUInt32,
                Pattern::Var("x"),
                Pattern::Const(Literal::I32(1)),
            ),
            |env: &MatchEnv, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::EqZInt32, *x, Type::I32))
            },
        );

        // x >=u 1 -> x != 0
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::GeUInt32,
                Pattern::Var("x"),
                Pattern::Const(Literal::I32(1)),
            ),
            |env: &MatchEnv, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                let zero = builder.const_(Literal::I32(0));
                Some(builder.binary(BinaryOp::NeInt32, *x, zero, Type::I32))
            },
        );

        // x <u MAX_INT -> x != -1
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::LtUInt32,
                Pattern::Var("x"),
                Pattern::Const(Literal::I32(-1)),
            ),
            |env: &MatchEnv, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                let minus_one = builder.const_(Literal::I32(-1));
                Some(builder.binary(BinaryOp::NeInt32, *x, minus_one, Type::I32))
            },
        );

        // x >=u MAX_INT -> x == -1
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::GeUInt32,
                Pattern::Var("x"),
                Pattern::Const(Literal::I32(-1)),
            ),
            |env: &MatchEnv, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                let minus_one = builder.const_(Literal::I32(-1));
                Some(builder.binary(BinaryOp::EqInt32, *x, minus_one, Type::I32))
            },
        );

        // --- I64 Versions ---

        // x == 0 -> eqz(x)
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::EqInt64,
                Pattern::Var("x"),
                Pattern::Const(Literal::I64(0)),
            ),
            |env: &MatchEnv, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::EqZInt64, *x, Type::I32)) // Note: EqZ returns I32
            },
        );

        // --- Boundary Folding ---

        // x < C  =>  x <= C-1 (if C is not MIN)
        // x > C  =>  x >= C+1 (if C is not MAX)
        // These can be useful for canonicalization.
        // Actually, C++ code often prefers < or <= depending on the context.
        // Let's add rules that convert < Constant to <= (Constant - 1) if it helps.

        matcher.add_rule(
            Pattern::binary(BinaryOp::LtSInt32, Pattern::Var("x"), Pattern::AnyConst),
            |env, bump| {
                let x = env.get("x")?;
                let c = env.get_const("right")?.get_i32();
                if c > i32::MIN {
                    let builder = IrBuilder::new(bump);
                    return Some(builder.binary(
                        BinaryOp::LeSInt32,
                        *x,
                        builder.const_(Literal::I32(c - 1)),
                        Type::I32,
                    ));
                }
                None
            },
        );

        matcher.add_rule(
            Pattern::binary(BinaryOp::GtSInt32, Pattern::Var("x"), Pattern::AnyConst),
            |env, bump| {
                let x = env.get("x")?;
                let c = env.get_const("right")?.get_i32();
                if c < i32::MAX {
                    let builder = IrBuilder::new(bump);
                    return Some(builder.binary(
                        BinaryOp::GeSInt32,
                        *x,
                        builder.const_(Literal::I32(c + 1)),
                        Type::I32,
                    ));
                }
                None
            },
        );

        // --- Unsigned Comparisons with Zero ---

        // unsigned(x) >= 0 -> i32(1)
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::GeUInt32,
                Pattern::Var("x"),
                Pattern::Const(Literal::I32(0)),
            ),
            |env: &MatchEnv, bump| {
                let _x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.const_(Literal::I32(1)))
            },
        );

        // unsigned(x) < 0 -> i32(0)
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::LtUInt32,
                Pattern::Var("x"),
                Pattern::Const(Literal::I32(0)),
            ),
            |env: &MatchEnv, bump| {
                let _x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.const_(Literal::I32(0)))
            },
        );

        // unsigned(x) >= 0 -> i64(1)
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::GeUInt64,
                Pattern::Var("x"),
                Pattern::Const(Literal::I64(0)),
            ),
            |env: &MatchEnv, bump| {
                let _x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.const_(Literal::I32(1)))
            },
        );

        // unsigned(x) < 0 -> i64(0)
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::LtUInt64,
                Pattern::Var("x"),
                Pattern::Const(Literal::I64(0)),
            ),
            |env: &MatchEnv, bump| {
                let _x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.const_(Literal::I32(0)))
            },
        );

        // eqz(x - y)  =>  x == y
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::EqZInt32,
                Pattern::binary(BinaryOp::SubInt32, Pattern::Var("x"), Pattern::Var("y")),
            ),
            |env: &MatchEnv, bump| {
                let x = env.get("x")?;
                let y = env.get("y")?;
                let builder = IrBuilder::new(bump);
                Some(builder.binary(BinaryOp::EqInt32, *x, *y, Type::I32))
            },
        );

        // eqz(x + C)  =>  x == -C
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::EqZInt32,
                Pattern::binary(
                    BinaryOp::AddInt32,
                    Pattern::Var("x"),
                    Pattern::Var("c_expr"),
                ),
            ),
            |env: &MatchEnv, bump| {
                let x = env.get("x")?;
                let c_expr = env.get("c_expr")?;
                if let ExpressionKind::Const(c_literal) = c_expr.kind {
                    let builder = IrBuilder::new(bump);
                    return Some(builder.binary(
                        BinaryOp::EqInt32,
                        *x,
                        builder.const_(c_literal.neg()),
                        Type::I32,
                    ));
                }
                None
            },
        );
    }

    fn register_algebraic_identities(matcher: &mut PatternMatcher) {
        // --- AddInt32 ---

        // x + 0 -> x
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::AddInt32,
                Pattern::Var("x"),
                Pattern::Const(Literal::I32(0)),
            ),
            |env: &MatchEnv, _| env.get("x").copied(),
        );

        // 0 + x -> x
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::AddInt32,
                Pattern::Const(Literal::I32(0)),
                Pattern::Var("x"),
            ),
            |env: &MatchEnv, _| env.get("x").copied(),
        );

        // --- MulInt32 ---

        // x * 1 -> x
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::MulInt32,
                Pattern::Var("x"),
                Pattern::Const(Literal::I32(1)),
            ),
            |env: &MatchEnv, _| env.get("x").copied(),
        );

        // 1 * x -> x
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::MulInt32,
                Pattern::Const(Literal::I32(1)),
                Pattern::Var("x"),
            ),
            |env: &MatchEnv, _| env.get("x").copied(),
        );

        // --- SubInt32 ---

        // x - 0 -> x
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::SubInt32,
                Pattern::Var("x"),
                Pattern::Const(Literal::I32(0)),
            ),
            |env: &MatchEnv, _| env.get("x").copied(),
        );

        // --- AndInt32 ---

        // x & -1 -> x (Identity)
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::AndInt32,
                Pattern::Var("x"),
                Pattern::Const(Literal::I32(-1)),
            ),
            |env: &MatchEnv, _| env.get("x").copied(),
        );

        // -1 & x -> x
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::AndInt32,
                Pattern::Const(Literal::I32(-1)),
                Pattern::Var("x"),
            ),
            |env: &MatchEnv, _| env.get("x").copied(),
        );

        // --- OrInt32 ---

        // x | 0 -> x
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::OrInt32,
                Pattern::Var("x"),
                Pattern::Const(Literal::I32(0)),
            ),
            |env: &MatchEnv, _| env.get("x").copied(),
        );

        // 0 | x -> x
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::OrInt32,
                Pattern::Const(Literal::I32(0)),
                Pattern::Var("x"),
            ),
            |env: &MatchEnv, _| env.get("x").copied(),
        );

        // --- XorInt32 ---

        // x ^ 0 -> x
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::XorInt32,
                Pattern::Var("x"),
                Pattern::Const(Literal::I32(0)),
            ),
            |env: &MatchEnv, _| env.get("x").copied(),
        );

        // 0 ^ x -> x
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::XorInt32,
                Pattern::Const(Literal::I32(0)),
                Pattern::Var("x"),
            ),
            |env: &MatchEnv, _| env.get("x").copied(),
        );

        // --- MulInt32 (by 0) ---

        // x * 0 -> 0
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::MulInt32,
                Pattern::Var("x"),
                Pattern::Const(Literal::I32(0)),
            ),
            |env: &MatchEnv, bump| {
                let _x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.const_(Literal::I32(0)))
            },
        );

        // 0 * x -> 0
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::MulInt32,
                Pattern::Const(Literal::I32(0)),
                Pattern::Var("x"),
            ),
            |env: &MatchEnv, bump| {
                let _x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.const_(Literal::I32(0)))
            },
        );

        // --- AndInt32 (with 0) ---

        // x & 0 -> 0
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::AndInt32,
                Pattern::Var("x"),
                Pattern::Const(Literal::I32(0)),
            ),
            |env: &MatchEnv, bump| {
                let _x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.const_(Literal::I32(0)))
            },
        );

        // 0 & x -> 0
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::AndInt32,
                Pattern::Const(Literal::I32(0)),
                Pattern::Var("x"),
            ),
            |env: &MatchEnv, bump| {
                let _x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.const_(Literal::I32(0)))
            },
        );

        // --- OrInt32 (x | x) ---

        // x | x -> x
        matcher.add_rule(
            Pattern::binary(BinaryOp::OrInt32, Pattern::Var("x"), Pattern::Var("x")),
            |env: &MatchEnv, _| env.get("x").copied(),
        );

        // --- XorInt32 (x ^ x) ---

        // x ^ x -> 0
        matcher.add_rule(
            Pattern::binary(BinaryOp::XorInt32, Pattern::Var("x"), Pattern::Var("x")),
            |_env: &MatchEnv, bump| {
                let builder = IrBuilder::new(bump);
                Some(builder.const_(Literal::I32(0)))
            },
        );

        // --- Double Negation ---

        // !(!(!x))) -> !x
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::EqZInt32,
                Pattern::unary(
                    UnaryOp::EqZInt32,
                    Pattern::unary(UnaryOp::EqZInt32, Pattern::Var("x")),
                ),
            ),
            |env: &MatchEnv, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::EqZInt32, *x, Type::I32))
            },
        );

        // !!x -> x != 0 (normalized boolean)
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::EqZInt32,
                Pattern::unary(UnaryOp::EqZInt32, Pattern::Var("x")),
            ),
            |env: &MatchEnv, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.binary(
                    BinaryOp::NeInt32,
                    *x,
                    builder.const_(Literal::I32(0)),
                    Type::I32,
                ))
            },
        );

        // --- Commutative Operator Normalization ---

        // Const(C) + Var(X) -> Var(X) + Const(C)
        matcher.add_rule(
            Pattern::binary(BinaryOp::AddInt32, Pattern::AnyConst, Pattern::Var("x")),
            |env: &MatchEnv, bump| {
                let x = env.get("x")?;
                let c = env.get_const("left")?; // AnyConst captures into "left"

                let builder = IrBuilder::new(bump);
                Some(builder.binary(BinaryOp::AddInt32, *x, builder.const_(c), Type::I32))
            },
        );

        // Const(C) * Var(X) -> Var(X) * Const(C)
        matcher.add_rule(
            Pattern::binary(BinaryOp::MulInt32, Pattern::AnyConst, Pattern::Var("x")),
            |env: &MatchEnv, bump| {
                let x = env.get("x")?;
                let c = env.get_const("left")?;

                let builder = IrBuilder::new(bump);
                Some(builder.binary(BinaryOp::MulInt32, *x, builder.const_(c), Type::I32))
            },
        );

        // Const(C) & Var(X) -> Var(X) & Const(C)
        matcher.add_rule(
            Pattern::binary(BinaryOp::AndInt32, Pattern::AnyConst, Pattern::Var("x")),
            |env: &MatchEnv, bump| {
                let x = env.get("x")?;
                let c = env.get_const("left")?;

                let builder = IrBuilder::new(bump);
                Some(builder.binary(BinaryOp::AndInt32, *x, builder.const_(c), Type::I32))
            },
        );

        // Const(C) | Var(X) -> Var(X) | Const(C)
        matcher.add_rule(
            Pattern::binary(BinaryOp::OrInt32, Pattern::AnyConst, Pattern::Var("x")),
            |env: &MatchEnv, bump| {
                let x = env.get("x")?;
                let c = env.get_const("left")?;

                let builder = IrBuilder::new(bump);
                Some(builder.binary(BinaryOp::OrInt32, *x, builder.const_(c), Type::I32))
            },
        );

        // Const(C) ^ Var(X) -> Var(X) ^ Const(C)
        matcher.add_rule(
            Pattern::binary(BinaryOp::XorInt32, Pattern::AnyConst, Pattern::Var("x")),
            |env: &MatchEnv, bump| {
                let x = env.get("x")?;
                let c = env.get_const("left")?;

                let builder = IrBuilder::new(bump);
                Some(builder.binary(BinaryOp::XorInt32, *x, builder.const_(c), Type::I32))
            },
        );

        // Const(C) == Var(X) -> Var(X) == Const(C)
        matcher.add_rule(
            Pattern::binary(BinaryOp::EqInt32, Pattern::AnyConst, Pattern::Var("x")),
            |env: &MatchEnv, bump| {
                let x = env.get("x")?;
                let c = env.get_const("left")?;

                let builder = IrBuilder::new(bump);
                Some(builder.binary(BinaryOp::EqInt32, *x, builder.const_(c), Type::I32))
            },
        );

        // Const(C) != Var(X) -> Var(X) != Const(C)
        matcher.add_rule(
            Pattern::binary(BinaryOp::NeInt32, Pattern::AnyConst, Pattern::Var("x")),
            |env: &MatchEnv, bump| {
                let x = env.get("x")?;
                let c = env.get_const("left")?;

                let builder = IrBuilder::new(bump);
                Some(builder.binary(BinaryOp::NeInt32, *x, builder.const_(c), Type::I32))
            },
        );

        // --- AddInt64 ---

        // Const(C) + Var(X) -> Var(X) + Const(C)
        matcher.add_rule(
            Pattern::binary(BinaryOp::AddInt64, Pattern::AnyConst, Pattern::Var("x")),
            |env: &MatchEnv, bump| {
                let x = env.get("x")?;
                let c = env.get_const("left")?;

                let builder = IrBuilder::new(bump);
                Some(builder.binary(BinaryOp::AddInt64, *x, builder.const_(c), Type::I64))
            },
        );

        // --- MulInt64 ---

        // Const(C) * Var(X) -> Var(X) * Const(C)
        matcher.add_rule(
            Pattern::binary(BinaryOp::MulInt64, Pattern::AnyConst, Pattern::Var("x")),
            |env: &MatchEnv, bump| {
                let x = env.get("x")?;
                let c = env.get_const("left")?;

                let builder = IrBuilder::new(bump);
                Some(builder.binary(BinaryOp::MulInt64, *x, builder.const_(c), Type::I64))
            },
        );

        // --- AndInt64 ---

        // Const(C) & Var(X) -> Var(X) & Const(C)
        matcher.add_rule(
            Pattern::binary(BinaryOp::AndInt64, Pattern::AnyConst, Pattern::Var("x")),
            |env: &MatchEnv, bump| {
                let x = env.get("x")?;
                let c = env.get_const("left")?;

                let builder = IrBuilder::new(bump);
                Some(builder.binary(BinaryOp::AndInt64, *x, builder.const_(c), Type::I64))
            },
        );

        // --- OrInt64 ---

        // Const(C) | Var(X) -> Var(X) | Const(C)
        matcher.add_rule(
            Pattern::binary(BinaryOp::OrInt64, Pattern::AnyConst, Pattern::Var("x")),
            |env: &MatchEnv, bump| {
                let x = env.get("x")?;
                let c = env.get_const("left")?;

                let builder = IrBuilder::new(bump);
                Some(builder.binary(BinaryOp::OrInt64, *x, builder.const_(c), Type::I64))
            },
        );

        // --- XorInt64 ---

        // Const(C) ^ Var(X) -> Var(X) ^ Const(C)
        matcher.add_rule(
            Pattern::binary(BinaryOp::XorInt64, Pattern::AnyConst, Pattern::Var("x")),
            |env: &MatchEnv, bump| {
                let x = env.get("x")?;
                let c = env.get_const("left")?;

                let builder = IrBuilder::new(bump);
                Some(builder.binary(BinaryOp::XorInt64, *x, builder.const_(c), Type::I64))
            },
        );

        // --- EqInt64 ---

        // Const(C) == Var(X) -> Var(X) == Const(C)
        matcher.add_rule(
            Pattern::binary(BinaryOp::EqInt64, Pattern::AnyConst, Pattern::Var("x")),
            |env: &MatchEnv, bump| {
                let x = env.get("x")?;
                let c = env.get_const("left")?;

                let builder = IrBuilder::new(bump);
                Some(builder.binary(BinaryOp::EqInt64, *x, builder.const_(c), Type::I64))
            },
        );

        // --- NeInt64 ---

        // Const(C) != Var(X) -> Var(X) != Const(C)
        matcher.add_rule(
            Pattern::binary(BinaryOp::NeInt64, Pattern::AnyConst, Pattern::Var("x")),
            |env: &MatchEnv, bump| {
                let x = env.get("x")?;
                let c = env.get_const("left")?;

                let builder = IrBuilder::new(bump);
                Some(builder.binary(BinaryOp::NeInt64, *x, builder.const_(c), Type::I64))
            },
        );

        // --- AddFloat32 ---

        // x + 0.0 -> x
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::AddFloat32,
                Pattern::Var("x"),
                Pattern::Const(Literal::F32(0.0)),
            ),
            |env: &MatchEnv, _| env.get("x").copied(),
        );

        // 0.0 + x -> x
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::AddFloat32,
                Pattern::Const(Literal::F32(0.0)),
                Pattern::Var("x"),
            ),
            |env: &MatchEnv, _| env.get("x").copied(),
        );

        // --- AddFloat64 ---

        // x + 0.0 -> x
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::AddFloat64,
                Pattern::Var("x"),
                Pattern::Const(Literal::F64(0.0)),
            ),
            |env: &MatchEnv, _| env.get("x").copied(),
        );

        // 0.0 + x -> x
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::AddFloat64,
                Pattern::Const(Literal::F64(0.0)),
                Pattern::Var("x"),
            ),
            |env: &MatchEnv, _| env.get("x").copied(),
        );

        // --- SubFloat32 ---

        // x - 0.0 -> x
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::SubFloat32,
                Pattern::Var("x"),
                Pattern::Const(Literal::F32(0.0)),
            ),
            |env: &MatchEnv, _| env.get("x").copied(),
        );

        // --- SubFloat64 ---

        // x - 0.0 -> x
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::SubFloat64,
                Pattern::Var("x"),
                Pattern::Const(Literal::F64(0.0)),
            ),
            |env: &MatchEnv, _| env.get("x").copied(),
        );

        // --- MulFloat32 ---

        // x * 1.0 -> x
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::MulFloat32,
                Pattern::Var("x"),
                Pattern::Const(Literal::F32(1.0)),
            ),
            |env: &MatchEnv, _| env.get("x").copied(),
        );

        // 1.0 * x -> x
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::MulFloat32,
                Pattern::Const(Literal::F32(1.0)),
                Pattern::Var("x"),
            ),
            |env: &MatchEnv, _| env.get("x").copied(),
        );

        // --- MulFloat32 (by 0.0) ---

        // x * 0.0 -> 0.0
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::MulFloat32,
                Pattern::Var("x"),
                Pattern::Const(Literal::F32(0.0)),
            ),
            |env: &MatchEnv, bump| {
                let _x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.const_(Literal::F32(0.0)))
            },
        );

        // 0.0 * x -> 0.0
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::MulFloat32,
                Pattern::Const(Literal::F32(0.0)),
                Pattern::Var("x"),
            ),
            |env: &MatchEnv, bump| {
                let _x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.const_(Literal::F32(0.0)))
            },
        );

        // --- MulFloat64 (by 0.0) ---

        // x * 0.0 -> 0.0
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::MulFloat64,
                Pattern::Var("x"),
                Pattern::Const(Literal::F64(0.0)),
            ),
            |env: &MatchEnv, bump| {
                let _x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.const_(Literal::F64(0.0)))
            },
        );

        // 0.0 * x -> 0.0
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::MulFloat64,
                Pattern::Const(Literal::F64(0.0)),
                Pattern::Var("x"),
            ),
            |env: &MatchEnv, bump| {
                let _x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.const_(Literal::F64(0.0)))
            },
        );

        // --- Bitwise NOT Patterns ---

        // x ^ -1 -> ~x (Normalized form is x ^ -1)
        // ~~x -> x
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::XorInt32,
                Pattern::binary(
                    BinaryOp::XorInt32,
                    Pattern::Var("x"),
                    Pattern::Const(Literal::I32(-1)),
                ),
                Pattern::Const(Literal::I32(-1)),
            ),
            |env: &MatchEnv, _| env.get("x").copied(),
        );

        // --- Self Identities ---

        // x - x -> 0
        matcher.add_rule(
            Pattern::binary(BinaryOp::SubInt32, Pattern::Var("x"), Pattern::Var("x")),
            |_env: &MatchEnv, bump| {
                let builder = IrBuilder::new(bump);
                Some(builder.const_(Literal::I32(0)))
            },
        );

        // x & x -> x
        matcher.add_rule(
            Pattern::binary(BinaryOp::AndInt32, Pattern::Var("x"), Pattern::Var("x")),
            |env: &MatchEnv, _| env.get("x").copied(),
        );

        // x == x -> 1 (For integers)
        matcher.add_rule(
            Pattern::binary(BinaryOp::EqInt32, Pattern::Var("x"), Pattern::Var("x")),
            |_env: &MatchEnv, bump| {
                let builder = IrBuilder::new(bump);
                Some(builder.const_(Literal::I32(1)))
            },
        );

        // x != x -> 0 (For integers)
        matcher.add_rule(
            Pattern::binary(BinaryOp::NeInt32, Pattern::Var("x"), Pattern::Var("x")),
            |_env: &MatchEnv, bump| {
                let builder = IrBuilder::new(bump);
                Some(builder.const_(Literal::I32(0)))
            },
        );

        // --- Shift/Rotate Patterns ---

        // (x << C1) << C2 -> x << (C1 + C2)
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::ShlInt32,
                Pattern::binary(BinaryOp::ShlInt32, Pattern::Var("x"), Pattern::AnyConst),
                Pattern::AnyConst,
            ),
            |env: &MatchEnv, bump| {
                let x = env.get("x")?;
                let c1 = env.get_const("left")?.get_i32();
                let c2 = env.get_const("right")?.get_i32();
                let builder = IrBuilder::new(bump);
                Some(builder.binary(
                    BinaryOp::ShlInt32,
                    *x,
                    builder.const_(Literal::I32(c1.wrapping_add(c2))),
                    Type::I32,
                ))
            },
        );

        // --- SubInt64 ---

        // x - 0 -> x
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::SubInt64,
                Pattern::Var("x"),
                Pattern::Const(Literal::I64(0)),
            ),
            |env: &MatchEnv, _| env.get("x").copied(),
        );

        // --- Cast / Reinterpret Identities ---

        // wrap(extend_s(x)) -> x
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::WrapInt64,
                Pattern::unary(UnaryOp::ExtendSInt32, Pattern::Var("x")),
            ),
            |env, _| env.get("x").copied(),
        );

        // wrap(extend_u(x)) -> x
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::WrapInt64,
                Pattern::unary(UnaryOp::ExtendUInt32, Pattern::Var("x")),
            ),
            |env, _| env.get("x").copied(),
        );

        // extend_s(wrap(x)) -> extend_s32(x)
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::ExtendSInt32,
                Pattern::unary(UnaryOp::WrapInt64, Pattern::Var("x")),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::ExtendS32Int64, *x, Type::I64))
            },
        );

        // extend_u(wrap(x)) -> x & 0xffffffff
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::ExtendUInt32,
                Pattern::unary(UnaryOp::WrapInt64, Pattern::Var("x")),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                let mask = builder.const_(Literal::I64(0xffffffff));
                Some(builder.binary(BinaryOp::AndInt64, *x, mask, Type::I64))
            },
        );

        // extend_s(wrap(x)) -> extend_s(wrap(x)) - No change needed here, it's already in minimal form.
        // But what about extend_s(i32_const) ? That's constant folding, which we already have.

        // reinterpret_f32(reinterpret_i32(x)) -> x
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::ReinterpretInt32,
                Pattern::unary(UnaryOp::ReinterpretFloat32, Pattern::Var("x")),
            ),
            |env, _| env.get("x").copied(),
        );

        // reinterpret_i32(reinterpret_f32(x)) -> x
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::ReinterpretFloat32,
                Pattern::unary(UnaryOp::ReinterpretInt32, Pattern::Var("x")),
            ),
            |env, _| env.get("x").copied(),
        );

        // reinterpret_f64(reinterpret_i64(x)) -> x
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::ReinterpretInt64,
                Pattern::unary(UnaryOp::ReinterpretFloat64, Pattern::Var("x")),
            ),
            |env, _| env.get("x").copied(),
        );

        // reinterpret_i64(reinterpret_f64(x)) -> x
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::ReinterpretFloat64,
                Pattern::unary(UnaryOp::ReinterpretInt64, Pattern::Var("x")),
            ),
            |env, _| env.get("x").copied(),
        );

        // --- Sign Extension Identities ---

        // extend_s16(extend_s8(x)) -> extend_s8(x)
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::ExtendS16Int32,
                Pattern::unary(UnaryOp::ExtendS8Int32, Pattern::Var("x")),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::ExtendS8Int32, *x, Type::I32))
            },
        );

        // extend_s8(extend_s16(x)) -> extend_s8(x)
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::ExtendS8Int32,
                Pattern::unary(UnaryOp::ExtendS16Int32, Pattern::Var("x")),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::ExtendS8Int32, *x, Type::I32))
            },
        );

        // --- Redundant Sign Extensions ---

        // extend_s8(extend_s8(x)) -> extend_s8(x)
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::ExtendS8Int32,
                Pattern::unary(UnaryOp::ExtendS8Int32, Pattern::Var("x")),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::ExtendS8Int32, *x, Type::I32))
            },
        );

        matcher.add_rule(
            Pattern::unary(
                UnaryOp::ExtendS16Int32,
                Pattern::unary(UnaryOp::ExtendS16Int32, Pattern::Var("x")),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::ExtendS16Int32, *x, Type::I32))
            },
        );

        matcher.add_rule(
            Pattern::unary(
                UnaryOp::ExtendS8Int64,
                Pattern::unary(UnaryOp::ExtendS8Int64, Pattern::Var("x")),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::ExtendS8Int64, *x, Type::I64))
            },
        );

        matcher.add_rule(
            Pattern::unary(
                UnaryOp::ExtendS16Int64,
                Pattern::unary(UnaryOp::ExtendS16Int64, Pattern::Var("x")),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::ExtendS16Int64, *x, Type::I64))
            },
        );

        matcher.add_rule(
            Pattern::unary(
                UnaryOp::ExtendS32Int64,
                Pattern::unary(UnaryOp::ExtendS32Int64, Pattern::Var("x")),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::ExtendS32Int64, *x, Type::I64))
            },
        );

        // --- Shift to Sign Extension Folding ---

        // (x << 24) >>s 24 -> extend_s8(x)
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::ShrSInt32,
                Pattern::binary(
                    BinaryOp::ShlInt32,
                    Pattern::Var("x"),
                    Pattern::Const(Literal::I32(24)),
                ),
                Pattern::Const(Literal::I32(24)),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::ExtendS8Int32, *x, Type::I32))
            },
        );

        // (x << 16) >>s 16 -> extend_s16(x)
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::ShrSInt32,
                Pattern::binary(
                    BinaryOp::ShlInt32,
                    Pattern::Var("x"),
                    Pattern::Const(Literal::I32(16)),
                ),
                Pattern::Const(Literal::I32(16)),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::ExtendS16Int32, *x, Type::I32))
            },
        );

        // (x << 56) >>s 56 -> extend_s8(x) (i64)
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::ShrSInt64,
                Pattern::binary(
                    BinaryOp::ShlInt64,
                    Pattern::Var("x"),
                    Pattern::Const(Literal::I64(56)),
                ),
                Pattern::Const(Literal::I64(56)),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::ExtendS8Int64, *x, Type::I64))
            },
        );

        // (x << 48) >>s 48 -> extend_s16(x) (i64)
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::ShrSInt64,
                Pattern::binary(
                    BinaryOp::ShlInt64,
                    Pattern::Var("x"),
                    Pattern::Const(Literal::I64(48)),
                ),
                Pattern::Const(Literal::I64(48)),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::ExtendS16Int64, *x, Type::I64))
            },
        );

        // (x << 32) >>s 32 -> extend_s32(x) (i64)
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::ShrSInt64,
                Pattern::binary(
                    BinaryOp::ShlInt64,
                    Pattern::Var("x"),
                    Pattern::Const(Literal::I64(32)),
                ),
                Pattern::Const(Literal::I64(32)),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::ExtendS32Int64, *x, Type::I64))
            },
        );

        // --- Cross-Bit-Width Sign Extension Identities ---

        // i64.extend_s32(i64.extend_s16(x)) -> i64.extend_s16(x)
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::ExtendS32Int64,
                Pattern::unary(UnaryOp::ExtendS16Int64, Pattern::Var("x")),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::ExtendS16Int64, *x, Type::I64))
            },
        );

        // i64.extend_s32(i64.extend_s8(x)) -> i64.extend_s8(x)
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::ExtendS32Int64,
                Pattern::unary(UnaryOp::ExtendS8Int64, Pattern::Var("x")),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::ExtendS8Int64, *x, Type::I64))
            },
        );

        // i64.extend_s16(i64.extend_s8(x)) -> i64.extend_s8(x)
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::ExtendS16Int64,
                Pattern::unary(UnaryOp::ExtendS8Int64, Pattern::Var("x")),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::ExtendS8Int64, *x, Type::I64))
            },
        );

        // i64.extend_s8(i64.extend_s32(x)) -> i64.extend_s8(x)
        matcher.add_rule(
            Pattern::unary(
                UnaryOp::ExtendS8Int64,
                Pattern::unary(UnaryOp::ExtendS32Int64, Pattern::Var("x")),
            ),
            |env, bump| {
                let x = env.get("x")?;
                let builder = IrBuilder::new(bump);
                Some(builder.unary(UnaryOp::ExtendS8Int64, *x, Type::I64))
            },
        );
    }

    fn register_reassociation(matcher: &mut PatternMatcher) {
        // (x + C1) + C2 -> x + (C1 + C2)
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::AddInt32,
                Pattern::binary(BinaryOp::AddInt32, Pattern::Var("x"), Pattern::AnyConst),
                Pattern::AnyConst,
            ),
            |env: &MatchEnv, bump| {
                let x = env.get("x")?;
                let c1 = env.get_const("left")?.get_i32();
                let c2 = env.get_const("right")?.get_i32();
                let builder = IrBuilder::new(bump);
                Some(builder.binary(
                    BinaryOp::AddInt32,
                    *x,
                    builder.const_(Literal::I32(c1.wrapping_add(c2))),
                    Type::I32,
                ))
            },
        );

        // (x * C1) * C2 -> x * (C1 * C2)
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::MulInt32,
                Pattern::binary(BinaryOp::MulInt32, Pattern::Var("x"), Pattern::AnyConst),
                Pattern::AnyConst,
            ),
            |env: &MatchEnv, bump| {
                let x = env.get("x")?;
                let c1 = env.get_const("left")?.get_i32();
                let c2 = env.get_const("right")?.get_i32();
                let builder = IrBuilder::new(bump);
                Some(builder.binary(
                    BinaryOp::MulInt32,
                    *x,
                    builder.const_(Literal::I32(c1.wrapping_mul(c2))),
                    Type::I32,
                ))
            },
        );

        // (x & C1) & C2 -> x & (C1 & C2)
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::AndInt32,
                Pattern::binary(BinaryOp::AndInt32, Pattern::Var("x"), Pattern::AnyConst),
                Pattern::AnyConst,
            ),
            |env: &MatchEnv, bump| {
                let x = env.get("x")?;
                let c1 = env.get_const("left")?.get_i32();
                let c2 = env.get_const("right")?.get_i32();
                let builder = IrBuilder::new(bump);
                Some(builder.binary(
                    BinaryOp::AndInt32,
                    *x,
                    builder.const_(Literal::I32(c1 & c2)),
                    Type::I32,
                ))
            },
        );

        // (x | C1) | C2 -> x | (C1 | C2)
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::OrInt32,
                Pattern::binary(BinaryOp::OrInt32, Pattern::Var("x"), Pattern::AnyConst),
                Pattern::AnyConst,
            ),
            |env: &MatchEnv, bump| {
                let x = env.get("x")?;
                let c1 = env.get_const("left")?.get_i32();
                let c2 = env.get_const("right")?.get_i32();
                let builder = IrBuilder::new(bump);
                Some(builder.binary(
                    BinaryOp::OrInt32,
                    *x,
                    builder.const_(Literal::I32(c1 | c2)),
                    Type::I32,
                ))
            },
        );

        // (x ^ C1) ^ C2 -> x ^ (C1 ^ C2)
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::XorInt32,
                Pattern::binary(BinaryOp::XorInt32, Pattern::Var("x"), Pattern::AnyConst),
                Pattern::AnyConst,
            ),
            |env: &MatchEnv, bump| {
                let x = env.get("x")?;
                let c1 = env.get_const("left")?.get_i32();
                let c2 = env.get_const("right")?.get_i32();
                let builder = IrBuilder::new(bump);
                Some(builder.binary(
                    BinaryOp::XorInt32,
                    *x,
                    builder.const_(Literal::I32(c1 ^ c2)),
                    Type::I32,
                ))
            },
        );

        // --- Multi-Operation Identities ---

        // (x + y) - x -> y
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::SubInt32,
                Pattern::binary(BinaryOp::AddInt32, Pattern::Var("x"), Pattern::Var("y")),
                Pattern::Var("x"),
            ),
            |env, _| env.get("y").copied(),
        );

        // (x + y) - y -> x
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::SubInt32,
                Pattern::binary(BinaryOp::AddInt32, Pattern::Var("x"), Pattern::Var("y")),
                Pattern::Var("y"),
            ),
            |env, _| env.get("x").copied(),
        );

        // (x - y) + y -> x
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::AddInt32,
                Pattern::binary(BinaryOp::SubInt32, Pattern::Var("x"), Pattern::Var("y")),
                Pattern::Var("y"),
            ),
            |env, _| env.get("x").copied(),
        );

        // y + (x - y) -> x
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::AddInt32,
                Pattern::Var("y"),
                Pattern::binary(BinaryOp::SubInt32, Pattern::Var("x"), Pattern::Var("y")),
            ),
            |env, _| env.get("x").copied(),
        );

        // x - (x - y) -> y
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::SubInt32,
                Pattern::Var("x"),
                Pattern::binary(BinaryOp::SubInt32, Pattern::Var("x"), Pattern::Var("y")),
            ),
            |env, _| env.get("y").copied(),
        );
    }

    fn register_strength_reduction(matcher: &mut PatternMatcher) {
        // x / 1 -> x (Unsigned)
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::DivUInt32,
                Pattern::Var("x"),
                Pattern::Const(Literal::I32(1)),
            ),
            |env: &MatchEnv, _| env.get("x").copied(),
        );

        // x / 1 -> x (Signed)
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::DivSInt32,
                Pattern::Var("x"),
                Pattern::Const(Literal::I32(1)),
            ),
            |env: &MatchEnv, _| env.get("x").copied(),
        );

        // x % 1 -> 0 (Unsigned)
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::RemUInt32,
                Pattern::Var("x"),
                Pattern::Const(Literal::I32(1)),
            ),
            |_env, bump| {
                let builder = IrBuilder::new(bump);
                Some(builder.const_(Literal::I32(0)))
            },
        );

        // x % 1 -> 0 (Signed)
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::RemSInt32,
                Pattern::Var("x"),
                Pattern::Const(Literal::I32(1)),
            ),
            |_env, bump| {
                let builder = IrBuilder::new(bump);
                Some(builder.const_(Literal::I32(0)))
            },
        );

        // --- Int64 versions ---

        // x / 1 -> x (Unsigned)
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::DivUInt64,
                Pattern::Var("x"),
                Pattern::Const(Literal::I64(1)),
            ),
            |env: &MatchEnv, _| env.get("x").copied(),
        );

        // x / 1 -> x (Signed)
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::DivSInt64,
                Pattern::Var("x"),
                Pattern::Const(Literal::I64(1)),
            ),
            |env: &MatchEnv, _| env.get("x").copied(),
        );

        // x % 1 -> 0 (Unsigned)
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::RemUInt64,
                Pattern::Var("x"),
                Pattern::Const(Literal::I64(1)),
            ),
            |_env, bump| {
                let builder = IrBuilder::new(bump);
                Some(builder.const_(Literal::I64(0)))
            },
        );

        // x % 1 -> 0 (Signed)
        matcher.add_rule(
            Pattern::binary(
                BinaryOp::RemSInt64,
                Pattern::Var("x"),
                Pattern::Const(Literal::I64(1)),
            ),
            |_env, bump| {
                let builder = IrBuilder::new(bump);
                Some(builder.const_(Literal::I64(0)))
            },
        );

        // x * 2^k -> x << k
        matcher.add_rule(
            Pattern::binary(BinaryOp::MulInt32, Pattern::Var("x"), Pattern::Var("c")),
            |env: &MatchEnv, bump| {
                let x = env.get("x")?;
                let c = env.get("c")?;
                if let ExpressionKind::Const(Literal::I32(val)) = c.kind {
                    if val > 0 && (val & (val - 1)) == 0 {
                        let k = val.trailing_zeros();
                        let builder = IrBuilder::new(bump);
                        let shift_amt = builder.const_(Literal::I32(k as i32));
                        // Clone x because we can't move out of env (shared ref) and can't easily deep clone without more work.
                        // But wait, Expression is in arena. We can just point to it?
                        // IrBuilder takes ExprRef which is Copy.
                        // But x is &ExprRef. *x is ExprRef.
                        return Some(builder.binary(BinaryOp::ShlInt32, *x, shift_amt, Type::I32));
                    }
                }
                None
            },
        );

        // x / 2^k -> x >> k (Unsigned)
        matcher.add_rule(
            Pattern::binary(BinaryOp::DivUInt32, Pattern::Var("x"), Pattern::Var("c")),
            |env: &MatchEnv, bump| {
                let x = env.get("x")?;
                let c = env.get("c")?;
                if let ExpressionKind::Const(Literal::I32(val)) = c.kind {
                    if val > 0 && (val & (val - 1)) == 0 {
                        let k = val.trailing_zeros();
                        let builder = IrBuilder::new(bump);
                        let shift_amt = builder.const_(Literal::I32(k as i32));
                        return Some(builder.binary(BinaryOp::ShrUInt32, *x, shift_amt, Type::I32));
                    }
                }
                None
            },
        );

        // x % 2^k -> x & (2^k - 1) (Unsigned)
        matcher.add_rule(
            Pattern::binary(BinaryOp::RemUInt32, Pattern::Var("x"), Pattern::Var("c")),
            |env: &MatchEnv, bump| {
                let x = env.get("x")?;
                let c = env.get("c")?;
                if let ExpressionKind::Const(Literal::I32(val)) = c.kind {
                    if val > 0 && (val & (val - 1)) == 0 {
                        let mask = val - 1;
                        let builder = IrBuilder::new(bump);
                        let mask_const = builder.const_(Literal::I32(mask));
                        return Some(builder.binary(BinaryOp::AndInt32, *x, mask_const, Type::I32));
                    }
                }
                None
            },
        );

        // --- Int64 Strength Reduction ---

        // x * 2^k -> x << k
        matcher.add_rule(
            Pattern::binary(BinaryOp::MulInt64, Pattern::Var("x"), Pattern::Var("c")),
            |env: &MatchEnv, bump| {
                let x = env.get("x")?;
                let c = env.get("c")?;
                if let ExpressionKind::Const(Literal::I64(val)) = c.kind {
                    if val > 0 && (val & (val - 1)) == 0 {
                        let k = val.trailing_zeros();
                        let builder = IrBuilder::new(bump);
                        let shift_amt = builder.const_(Literal::I64(k as i64));
                        return Some(builder.binary(BinaryOp::ShlInt64, *x, shift_amt, Type::I64));
                    }
                }
                None
            },
        );

        // x / 2^k -> x >> k (Unsigned)
        matcher.add_rule(
            Pattern::binary(BinaryOp::DivUInt64, Pattern::Var("x"), Pattern::Var("c")),
            |env: &MatchEnv, bump| {
                let x = env.get("x")?;
                let c = env.get("c")?;
                if let ExpressionKind::Const(Literal::I64(val)) = c.kind {
                    if val > 0 && (val & (val - 1)) == 0 {
                        let k = val.trailing_zeros();
                        let builder = IrBuilder::new(bump);
                        let shift_amt = builder.const_(Literal::I64(k as i64));
                        return Some(builder.binary(BinaryOp::ShrUInt64, *x, shift_amt, Type::I64));
                    }
                }
                None
            },
        );

        // x % 2^k -> x & (2^k - 1) (Unsigned)
        matcher.add_rule(
            Pattern::binary(BinaryOp::RemUInt64, Pattern::Var("x"), Pattern::Var("c")),
            |env: &MatchEnv, bump| {
                let x = env.get("x")?;
                let c = env.get("c")?;
                if let ExpressionKind::Const(Literal::I64(val)) = c.kind {
                    if val > 0 && (val & (val - 1)) == 0 {
                        let mask = val - 1;
                        let builder = IrBuilder::new(bump);
                        let mask_const = builder.const_(Literal::I64(mask));
                        return Some(builder.binary(BinaryOp::AndInt64, *x, mask_const, Type::I64));
                    }
                }
                None
            },
        );
    }
}

impl Pass for OptimizeInstructions {
    fn name(&self) -> &str {
        "optimize-instructions"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        let mut visitor = OptimizeInstructionsVisitor {
            matcher: &self.matcher,
            allocator: module.allocator,
        };

        for func in &mut module.functions {
            if let Some(body) = &mut func.body {
                visitor.visit(body);
            }
        }
    }
}

struct OptimizeInstructionsVisitor<'a, 'b> {
    matcher: &'b PatternMatcher,
    allocator: &'a bumpalo::Bump,
}

impl<'a, 'b> Visitor<'a> for OptimizeInstructionsVisitor<'a, 'b> {
    fn visit(&mut self, expr: &mut ExprRef<'a>) {
        // Bottom-up traversal: simplify children first
        self.visit_children(expr);

        // Try to simplify current expression repeatedly until no more simplifications are found
        let mut iterations = 0;
        const MAX_ITERATIONS: usize = 10;

        while iterations < MAX_ITERATIONS {
            // println!("Simplifying: {:?}", expr.kind);
            if let Some(new_expr) = self.matcher.simplify(*expr, self.allocator) {
                // println!("  -> Simplified to: {:?}", new_expr.kind);
                *expr = new_expr;
                expr.finalize(); // Recompute type

                // After replacing the current node, simplify the new subtree's children too.
                self.visit_children(expr);
                iterations += 1;
            } else {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::{ExpressionKind, IrBuilder};
    use crate::ir::{Literal, Type};
    use bumpalo::Bump;

    #[test]
    fn test_optimize_add_zero() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // (local.get 0) + 0
        let x = builder.local_get(0, Type::I32);
        let zero = builder.const_(Literal::I32(0));
        let add = builder.binary(BinaryOp::AddInt32, x, zero, Type::I32);

        let pass = OptimizeInstructions::new();
        let mut visitor = OptimizeInstructionsVisitor {
            matcher: &pass.matcher,
            allocator: &bump,
        };

        let mut expr_ref = add;
        visitor.visit(&mut expr_ref);

        // Should be replaced by local.get 0
        assert!(matches!(
            expr_ref.kind,
            ExpressionKind::LocalGet { index: 0, .. }
        ));
    }

    #[test]
    fn test_optimize_mul_one() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // 1 * (local.get 0)
        let x = builder.local_get(0, Type::I32);
        let one = builder.const_(Literal::I32(1));
        let mul = builder.binary(BinaryOp::MulInt32, one, x, Type::I32);

        let pass = OptimizeInstructions::new();
        let mut visitor = OptimizeInstructionsVisitor {
            matcher: &pass.matcher,
            allocator: &bump,
        };

        let mut expr_ref = mul;
        visitor.visit(&mut expr_ref);

        assert!(matches!(
            expr_ref.kind,
            ExpressionKind::LocalGet { index: 0, .. }
        ));
    }

    #[test]
    fn test_optimize_mul_zero() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // x * 0 -> 0
        let x = builder.local_get(0, Type::I32);
        let zero = builder.const_(Literal::I32(0));
        let mul = builder.binary(BinaryOp::MulInt32, x, zero, Type::I32);

        let pass = OptimizeInstructions::new();
        let mut visitor = OptimizeInstructionsVisitor {
            matcher: &pass.matcher,
            allocator: &bump,
        };

        let mut expr_ref = mul;
        visitor.visit(&mut expr_ref);

        assert!(matches!(
            expr_ref.kind,
            ExpressionKind::Const(Literal::I32(0))
        ));

        // 0 * x -> 0
        let mul_rev = builder.binary(BinaryOp::MulInt32, zero, x, Type::I32);
        let mut expr_ref_rev = mul_rev;
        visitor.visit(&mut expr_ref_rev);

        assert!(matches!(
            expr_ref_rev.kind,
            ExpressionKind::Const(Literal::I32(0))
        ));
    }

    #[test]
    fn test_optimize_and_zero() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // x & 0 -> 0
        let x = builder.local_get(0, Type::I32);
        let zero = builder.const_(Literal::I32(0));
        let and = builder.binary(BinaryOp::AndInt32, x, zero, Type::I32);

        let pass = OptimizeInstructions::new();
        let mut visitor = OptimizeInstructionsVisitor {
            matcher: &pass.matcher,
            allocator: &bump,
        };

        let mut expr_ref = and;
        visitor.visit(&mut expr_ref);

        assert!(matches!(
            expr_ref.kind,
            ExpressionKind::Const(Literal::I32(0))
        ));

        // 0 & x -> 0
        let and_rev = builder.binary(BinaryOp::AndInt32, zero, x, Type::I32);
        let mut expr_ref_rev = and_rev;
        visitor.visit(&mut expr_ref_rev);

        assert!(matches!(
            expr_ref_rev.kind,
            ExpressionKind::Const(Literal::I32(0))
        ));
    }

    #[test]
    fn test_optimize_or_self() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // x | x -> x
        let x = builder.local_get(0, Type::I32);
        let or = builder.binary(BinaryOp::OrInt32, x, x, Type::I32);

        let pass = OptimizeInstructions::new();
        let mut visitor = OptimizeInstructionsVisitor {
            matcher: &pass.matcher,
            allocator: &bump,
        };

        let mut expr_ref = or;
        visitor.visit(&mut expr_ref);

        assert!(matches!(
            expr_ref.kind,
            ExpressionKind::LocalGet { index: 0, .. }
        ));
    }

    #[test]
    fn test_optimize_xor_self() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // x ^ x -> 0
        let x = builder.local_get(0, Type::I32);
        let xor = builder.binary(BinaryOp::XorInt32, x, x, Type::I32);

        let pass = OptimizeInstructions::new();
        let mut visitor = OptimizeInstructionsVisitor {
            matcher: &pass.matcher,
            allocator: &bump,
        };

        let mut expr_ref = xor;
        visitor.visit(&mut expr_ref);

        assert!(matches!(
            expr_ref.kind,
            ExpressionKind::Const(Literal::I32(0))
        ));
    }

    #[test]
    fn test_optimize_double_negation() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // !!x -> x != 0
        let x = builder.local_get(0, Type::I32);
        let neg1 = builder.unary(UnaryOp::EqZInt32, x, Type::I32);
        let neg2 = builder.unary(UnaryOp::EqZInt32, neg1, Type::I32);

        let pass = OptimizeInstructions::new();
        let mut visitor = OptimizeInstructionsVisitor {
            matcher: &pass.matcher,
            allocator: &bump,
        };

        let mut expr_ref = neg2;
        visitor.visit(&mut expr_ref);

        if let ExpressionKind::Binary { op, right, .. } = &expr_ref.kind {
            assert_eq!(*op, BinaryOp::NeInt32);
            assert!(matches!(right.kind, ExpressionKind::Const(Literal::I32(0))));
        } else {
            panic!("Expected x != 0, got {:?}", expr_ref.kind);
        }
    }

    #[test]
    fn test_optimize_commutative_add() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // 10 + x -> x + 10
        let x = builder.local_get(0, Type::I32);
        let c10 = builder.const_(Literal::I32(10));
        let add = builder.binary(BinaryOp::AddInt32, c10, x, Type::I32);

        let pass = OptimizeInstructions::new();
        let mut visitor = OptimizeInstructionsVisitor {
            matcher: &pass.matcher,
            allocator: &bump,
        };

        let mut expr_ref = add;
        visitor.visit(&mut expr_ref);

        if let ExpressionKind::Binary { op, left, right } = expr_ref.kind {
            assert_eq!(op, BinaryOp::AddInt32);
            assert!(matches!(
                left.kind,
                ExpressionKind::LocalGet { index: 0, .. }
            ));
            assert!(matches!(
                right.kind,
                ExpressionKind::Const(Literal::I32(10))
            ));
        } else {
            panic!("Expected binary add with reordered operands");
        }
    }

    #[test]
    fn test_optimize_commutative_mul() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // 10 * x -> x * 10
        let x = builder.local_get(0, Type::I32);
        let c10 = builder.const_(Literal::I32(10));
        let mul = builder.binary(BinaryOp::MulInt32, c10, x, Type::I32);

        let pass = OptimizeInstructions::new();
        let mut visitor = OptimizeInstructionsVisitor {
            matcher: &pass.matcher,
            allocator: &bump,
        };

        let mut expr_ref = mul;
        visitor.visit(&mut expr_ref);

        if let ExpressionKind::Binary { op, left, right } = expr_ref.kind {
            assert_eq!(op, BinaryOp::MulInt32);
            assert!(matches!(
                left.kind,
                ExpressionKind::LocalGet { index: 0, .. }
            ));
            assert!(matches!(
                right.kind,
                ExpressionKind::Const(Literal::I32(10))
            ));
        } else {
            panic!("Expected binary mul with reordered operands");
        }
    }

    #[test]
    fn test_optimize_commutative_and() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // 10 & x -> x & 10
        let x = builder.local_get(0, Type::I32);
        let c10 = builder.const_(Literal::I32(10));
        let and = builder.binary(BinaryOp::AndInt32, c10, x, Type::I32);

        let pass = OptimizeInstructions::new();
        let mut visitor = OptimizeInstructionsVisitor {
            matcher: &pass.matcher,
            allocator: &bump,
        };

        let mut expr_ref = and;
        visitor.visit(&mut expr_ref);

        if let ExpressionKind::Binary { op, left, right } = expr_ref.kind {
            assert_eq!(op, BinaryOp::AndInt32);
            assert!(matches!(
                left.kind,
                ExpressionKind::LocalGet { index: 0, .. }
            ));
            assert!(matches!(
                right.kind,
                ExpressionKind::Const(Literal::I32(10))
            ));
        } else {
            panic!("Expected binary and with reordered operands");
        }
    }

    #[test]
    fn test_optimize_commutative_or() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // 10 | x -> x | 10
        let x = builder.local_get(0, Type::I32);
        let c10 = builder.const_(Literal::I32(10));
        let or = builder.binary(BinaryOp::OrInt32, c10, x, Type::I32);

        let pass = OptimizeInstructions::new();
        let mut visitor = OptimizeInstructionsVisitor {
            matcher: &pass.matcher,
            allocator: &bump,
        };

        let mut expr_ref = or;
        visitor.visit(&mut expr_ref);

        if let ExpressionKind::Binary { op, left, right } = expr_ref.kind {
            assert_eq!(op, BinaryOp::OrInt32);
            assert!(matches!(
                left.kind,
                ExpressionKind::LocalGet { index: 0, .. }
            ));
            assert!(matches!(
                right.kind,
                ExpressionKind::Const(Literal::I32(10))
            ));
        } else {
            panic!("Expected binary or with reordered operands");
        }
    }

    #[test]
    fn test_optimize_commutative_xor() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // 10 ^ x -> x ^ 10
        let x = builder.local_get(0, Type::I32);
        let c10 = builder.const_(Literal::I32(10));
        let xor = builder.binary(BinaryOp::XorInt32, c10, x, Type::I32);

        let pass = OptimizeInstructions::new();
        let mut visitor = OptimizeInstructionsVisitor {
            matcher: &pass.matcher,
            allocator: &bump,
        };

        let mut expr_ref = xor;
        visitor.visit(&mut expr_ref);

        if let ExpressionKind::Binary { op, left, right } = expr_ref.kind {
            assert_eq!(op, BinaryOp::XorInt32);
            assert!(matches!(
                left.kind,
                ExpressionKind::LocalGet { index: 0, .. }
            ));
            assert!(matches!(
                right.kind,
                ExpressionKind::Const(Literal::I32(10))
            ));
        } else {
            panic!("Expected binary xor with reordered operands");
        }
    }

    #[test]
    fn test_optimize_commutative_eq() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // 10 == x -> x == 10
        let x = builder.local_get(0, Type::I32);
        let c10 = builder.const_(Literal::I32(10));
        let eq = builder.binary(BinaryOp::EqInt32, c10, x, Type::I32);

        let pass = OptimizeInstructions::new();
        let mut visitor = OptimizeInstructionsVisitor {
            matcher: &pass.matcher,
            allocator: &bump,
        };

        let mut expr_ref = eq;
        visitor.visit(&mut expr_ref);

        if let ExpressionKind::Binary { op, left, right } = expr_ref.kind {
            assert_eq!(op, BinaryOp::EqInt32);
            assert!(matches!(
                left.kind,
                ExpressionKind::LocalGet { index: 0, .. }
            ));
            assert!(matches!(
                right.kind,
                ExpressionKind::Const(Literal::I32(10))
            ));
        } else {
            panic!("Expected binary eq with reordered operands");
        }
    }

    #[test]
    fn test_optimize_commutative_ne() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // 10 != x -> x != 10
        let x = builder.local_get(0, Type::I32);
        let c10 = builder.const_(Literal::I32(10));
        let ne = builder.binary(BinaryOp::NeInt32, c10, x, Type::I32);

        let pass = OptimizeInstructions::new();
        let mut visitor = OptimizeInstructionsVisitor {
            matcher: &pass.matcher,
            allocator: &bump,
        };

        let mut expr_ref = ne;
        visitor.visit(&mut expr_ref);

        if let ExpressionKind::Binary { op, left, right } = expr_ref.kind {
            assert_eq!(op, BinaryOp::NeInt32);
            assert!(matches!(
                left.kind,
                ExpressionKind::LocalGet { index: 0, .. }
            ));
            assert!(matches!(
                right.kind,
                ExpressionKind::Const(Literal::I32(10))
            ));
        } else {
            panic!("Expected binary ne with reordered operands");
        }
    }

    #[test]
    fn test_optimize_commutative_add_i64() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // 10L + x -> x + 10L
        let x = builder.local_get(0, Type::I64);
        let c10 = builder.const_(Literal::I64(10));
        let add = builder.binary(BinaryOp::AddInt64, c10, x, Type::I64);

        let pass = OptimizeInstructions::new();
        let mut visitor = OptimizeInstructionsVisitor {
            matcher: &pass.matcher,
            allocator: &bump,
        };

        let mut expr_ref = add;
        visitor.visit(&mut expr_ref);

        if let ExpressionKind::Binary { op, left, right } = expr_ref.kind {
            assert_eq!(op, BinaryOp::AddInt64);
            assert!(matches!(
                left.kind,
                ExpressionKind::LocalGet { index: 0, .. }
            ));
            assert!(matches!(
                right.kind,
                ExpressionKind::Const(Literal::I64(10))
            ));
        } else {
            panic!("Expected binary add i64 with reordered operands");
        }
    }

    #[test]
    fn test_optimize_commutative_mul_i64() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // 10L * x -> x * 10L
        let x = builder.local_get(0, Type::I64);
        let c10 = builder.const_(Literal::I64(10));
        let mul = builder.binary(BinaryOp::MulInt64, c10, x, Type::I64);

        let pass = OptimizeInstructions::new();
        let mut visitor = OptimizeInstructionsVisitor {
            matcher: &pass.matcher,
            allocator: &bump,
        };

        let mut expr_ref = mul;
        visitor.visit(&mut expr_ref);

        if let ExpressionKind::Binary { op, left, right } = expr_ref.kind {
            assert_eq!(op, BinaryOp::MulInt64);
            assert!(matches!(
                left.kind,
                ExpressionKind::LocalGet { index: 0, .. }
            ));
            assert!(matches!(
                right.kind,
                ExpressionKind::Const(Literal::I64(10))
            ));
        } else {
            panic!("Expected binary mul i64 with reordered operands");
        }
    }

    #[test]
    fn test_optimize_commutative_and_i64() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // 10L & x -> x & 10L
        let x = builder.local_get(0, Type::I64);
        let c10 = builder.const_(Literal::I64(10));
        let and = builder.binary(BinaryOp::AndInt64, c10, x, Type::I64);

        let pass = OptimizeInstructions::new();
        let mut visitor = OptimizeInstructionsVisitor {
            matcher: &pass.matcher,
            allocator: &bump,
        };

        let mut expr_ref = and;
        visitor.visit(&mut expr_ref);

        if let ExpressionKind::Binary { op, left, right } = expr_ref.kind {
            assert_eq!(op, BinaryOp::AndInt64);
            assert!(matches!(
                left.kind,
                ExpressionKind::LocalGet { index: 0, .. }
            ));
            assert!(matches!(
                right.kind,
                ExpressionKind::Const(Literal::I64(10))
            ));
        } else {
            panic!("Expected binary and i64 with reordered operands");
        }
    }

    #[test]
    fn test_optimize_commutative_or_i64() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // 10L | x -> x | 10L
        let x = builder.local_get(0, Type::I64);
        let c10 = builder.const_(Literal::I64(10));
        let or = builder.binary(BinaryOp::OrInt64, c10, x, Type::I64);

        let pass = OptimizeInstructions::new();
        let mut visitor = OptimizeInstructionsVisitor {
            matcher: &pass.matcher,
            allocator: &bump,
        };

        let mut expr_ref = or;
        visitor.visit(&mut expr_ref);

        if let ExpressionKind::Binary { op, left, right } = expr_ref.kind {
            assert_eq!(op, BinaryOp::OrInt64);
            assert!(matches!(
                left.kind,
                ExpressionKind::LocalGet { index: 0, .. }
            ));
            assert!(matches!(
                right.kind,
                ExpressionKind::Const(Literal::I64(10))
            ));
        } else {
            panic!("Expected binary or i64 with reordered operands");
        }
    }

    #[test]
    fn test_optimize_commutative_xor_i64() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // 10L ^ x -> x ^ 10L
        let x = builder.local_get(0, Type::I64);
        let c10 = builder.const_(Literal::I64(10));
        let xor = builder.binary(BinaryOp::XorInt64, c10, x, Type::I64);

        let pass = OptimizeInstructions::new();
        let mut visitor = OptimizeInstructionsVisitor {
            matcher: &pass.matcher,
            allocator: &bump,
        };

        let mut expr_ref = xor;
        visitor.visit(&mut expr_ref);

        if let ExpressionKind::Binary { op, left, right } = expr_ref.kind {
            assert_eq!(op, BinaryOp::XorInt64);
            assert!(matches!(
                left.kind,
                ExpressionKind::LocalGet { index: 0, .. }
            ));
            assert!(matches!(
                right.kind,
                ExpressionKind::Const(Literal::I64(10))
            ));
        } else {
            panic!("Expected binary xor i64 with reordered operands");
        }
    }

    #[test]
    fn test_optimize_commutative_eq_i64() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // 10L == x -> x == 10L
        let x = builder.local_get(0, Type::I64);
        let c10 = builder.const_(Literal::I64(10));
        let eq = builder.binary(BinaryOp::EqInt64, c10, x, Type::I64);

        let pass = OptimizeInstructions::new();
        let mut visitor = OptimizeInstructionsVisitor {
            matcher: &pass.matcher,
            allocator: &bump,
        };

        let mut expr_ref = eq;
        visitor.visit(&mut expr_ref);

        if let ExpressionKind::Binary { op, left, right } = expr_ref.kind {
            assert_eq!(op, BinaryOp::EqInt64);
            assert!(matches!(
                left.kind,
                ExpressionKind::LocalGet { index: 0, .. }
            ));
            assert!(matches!(
                right.kind,
                ExpressionKind::Const(Literal::I64(10))
            ));
        } else {
            panic!("Expected binary eq i64 with reordered operands");
        }
    }

    #[test]
    fn test_optimize_commutative_ne_i64() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // 10L != x -> x != 10L
        let x = builder.local_get(0, Type::I64);
        let c10 = builder.const_(Literal::I64(10));
        let ne = builder.binary(BinaryOp::NeInt64, c10, x, Type::I64);

        let pass = OptimizeInstructions::new();
        let mut visitor = OptimizeInstructionsVisitor {
            matcher: &pass.matcher,
            allocator: &bump,
        };

        let mut expr_ref = ne;
        visitor.visit(&mut expr_ref);

        if let ExpressionKind::Binary { op, left, right } = expr_ref.kind {
            assert_eq!(op, BinaryOp::NeInt64);
            assert!(matches!(
                left.kind,
                ExpressionKind::LocalGet { index: 0, .. }
            ));
            assert!(matches!(
                right.kind,
                ExpressionKind::Const(Literal::I64(10))
            ));
        } else {
            panic!("Expected binary ne i64 with reordered operands");
        }
    }

    #[test]
    fn test_optimize_sub_zero_i64() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // x - 0 -> x
        let x = builder.local_get(0, Type::I64);
        let zero = builder.const_(Literal::I64(0));
        let sub = builder.binary(BinaryOp::SubInt64, x, zero, Type::I64);

        let pass = OptimizeInstructions::new();
        let mut visitor = OptimizeInstructionsVisitor {
            matcher: &pass.matcher,
            allocator: &bump,
        };

        let mut expr_ref = sub;
        visitor.visit(&mut expr_ref);

        assert!(matches!(
            expr_ref.kind,
            ExpressionKind::LocalGet { index: 0, .. }
        ));
    }

    #[test]
    fn test_optimize_f32_add_nan() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // nan + 1.0 -> should not fold to a constant normally if we want to preserve NaN properties or if the pass avoids folding NaNs
        let nan = builder.const_(Literal::F32(f32::NAN));
        let one = builder.const_(Literal::F32(1.0));
        let add = builder.binary(BinaryOp::AddFloat32, nan, one, Type::F32);

        let pass = OptimizeInstructions::new();
        let mut visitor = OptimizeInstructionsVisitor {
            matcher: &pass.matcher,
            allocator: &bump,
        };

        let mut expr_ref = add;
        visitor.visit(&mut expr_ref);

        // Based on eval_binary_op, and the fact that 1.0 is not captured by Identities (only 0.0 for add)
        // Const folding rule: Pattern::binary(PatternOp::AnyOp, Pattern::AnyConst, Pattern::AnyConst)
        // eval_binary_op for AddFloat32 just does left + right.
        // If it resulted in NaN, it currently DOES fold unless it's Div.

        // Wait, I should check what the current eval_binary_op does for AddFloat32.
        // AddFloat32 => Some(Literal::F32(left.get_f32() + right.get_f32())),
        // So it folds it to a NaN constant.

        assert!(matches!(expr_ref.kind, ExpressionKind::Const(Literal::F32(v)) if v.is_nan()));
    }

    #[test]
    fn test_optimize_f32_div_zero() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // 1.0 / 0.0 -> should return None in eval_binary_op and not fold
        let one = builder.const_(Literal::F32(1.0));
        let zero = builder.const_(Literal::F32(0.0));
        let div = builder.binary(BinaryOp::DivFloat32, one, zero, Type::F32);

        let pass = OptimizeInstructions::new();
        let mut visitor = OptimizeInstructionsVisitor {
            matcher: &pass.matcher,
            allocator: &bump,
        };

        let mut expr_ref = div;
        visitor.visit(&mut expr_ref);

        // Should NOT be folded
        assert!(matches!(
            expr_ref.kind,
            ExpressionKind::Binary {
                op: BinaryOp::DivFloat32,
                ..
            }
        ));
    }

    #[test]
    fn test_optimize_strength_reduction_mul() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // x * 4 -> x << 2
        let x = builder.local_get(0, Type::I32);
        let c4 = builder.const_(Literal::I32(4));
        let mul = builder.binary(BinaryOp::MulInt32, x, c4, Type::I32);

        let pass = OptimizeInstructions::new();
        let mut visitor = OptimizeInstructionsVisitor {
            matcher: &pass.matcher,
            allocator: &bump,
        };

        let mut expr_ref = mul;
        visitor.visit(&mut expr_ref);

        if let ExpressionKind::Binary { op, left: _, right } = expr_ref.kind {
            assert_eq!(op, BinaryOp::ShlInt32);
            assert!(matches!(right.kind, ExpressionKind::Const(Literal::I32(2))));
        } else {
            panic!("Expected shift left for multiplication by power of 2");
        }
    }

    #[test]
    fn test_optimize_comparison_eqz() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // x == 0 -> eqz(x)
        let x = builder.local_get(0, Type::I32);
        let zero = builder.const_(Literal::I32(0));
        let eq = builder.binary(BinaryOp::EqInt32, x, zero, Type::I32);

        let pass = OptimizeInstructions::new();
        let mut visitor = OptimizeInstructionsVisitor {
            matcher: &pass.matcher,
            allocator: &bump,
        };

        let mut expr_ref = eq;
        visitor.visit(&mut expr_ref);

        assert!(matches!(
            expr_ref.kind,
            ExpressionKind::Unary {
                op: UnaryOp::EqZInt32,
                ..
            }
        ));
    }

    #[test]
    fn test_optimize_reassociation() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // (x + 10) + 20 -> x + 30
        let x = builder.local_get(0, Type::I32);
        let c10 = builder.const_(Literal::I32(10));
        let c20 = builder.const_(Literal::I32(20));
        let inner_add = builder.binary(BinaryOp::AddInt32, x, c10, Type::I32);
        let outer_add = builder.binary(BinaryOp::AddInt32, inner_add, c20, Type::I32);

        let pass = OptimizeInstructions::new();
        let mut visitor = OptimizeInstructionsVisitor {
            matcher: &pass.matcher,
            allocator: &bump,
        };

        let mut expr_ref = outer_add;
        visitor.visit(&mut expr_ref);

        if let ExpressionKind::Binary { op, left, right } = expr_ref.kind {
            assert_eq!(op, BinaryOp::AddInt32);
            assert!(matches!(
                left.kind,
                ExpressionKind::LocalGet { index: 0, .. }
            ));
            assert!(matches!(
                right.kind,
                ExpressionKind::Const(Literal::I32(30))
            ));
        } else {
            panic!("Expected reassociated add (x + 30)");
        }
    }

    #[test]
    fn test_optimize_shift_chaining() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // (x << 2) << 3 -> x << 5
        let x = builder.local_get(0, Type::I32);
        let c2 = builder.const_(Literal::I32(2));
        let c3 = builder.const_(Literal::I32(3));
        let inner_shl = builder.binary(BinaryOp::ShlInt32, x, c2, Type::I32);
        let outer_shl = builder.binary(BinaryOp::ShlInt32, inner_shl, c3, Type::I32);

        let pass = OptimizeInstructions::new();
        let mut visitor = OptimizeInstructionsVisitor {
            matcher: &pass.matcher,
            allocator: &bump,
        };

        let mut expr_ref = outer_shl;
        visitor.visit(&mut expr_ref);

        if let ExpressionKind::Binary { op, left, right } = expr_ref.kind {
            assert_eq!(op, BinaryOp::ShlInt32);
            assert!(matches!(
                left.kind,
                ExpressionKind::LocalGet { index: 0, .. }
            ));
            assert!(matches!(right.kind, ExpressionKind::Const(Literal::I32(5))));
        } else {
            panic!("Expected chained shift (x << 5)");
        }
    }

    #[test]
    fn test_optimize_self_sub() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // x - x -> 0
        let x = builder.local_get(0, Type::I32);
        let sub = builder.binary(BinaryOp::SubInt32, x, x, Type::I32);

        let pass = OptimizeInstructions::new();
        let mut visitor = OptimizeInstructionsVisitor {
            matcher: &pass.matcher,
            allocator: &bump,
        };

        let mut expr_ref = sub;
        visitor.visit(&mut expr_ref);

        assert!(matches!(
            expr_ref.kind,
            ExpressionKind::Const(Literal::I32(0))
        ));
    }
}
