use crate::collections::id_arena::IdArena;
use crate::ir::expression::{ExpressionKind, IrBuilder};
use crate::ir::{Literal, Type};
use std::time::Instant;

#[test]
fn test_deep_recursion_stress() {
    let mut arena = IdArena::new();
    let mut builder = IrBuilder::new(&mut arena);

    // Testing stack depth / arena behavior with deep chain
    let depth = 50_000;
    let mut expr = builder.const_(Literal::I32(0));

    for _ in 0..depth {
        expr = builder.unary(crate::ir::ops::UnaryOp::EqZInt32, expr, Type::I32);
    }

    assert_eq!(arena[expr].type_, Type::I32);
}

#[test]
fn test_wide_allocation_stress() {
    let mut arena = IdArena::new();
    let mut builder = IrBuilder::new(&mut arena);

    let width = 100_000;
    let mut list = Vec::with_capacity(width);

    for i in 0..width {
        list.push(builder.const_(Literal::I32(i as i32)));
    }

    let block = builder.block(None::<String>, list, Type::NONE);

    if let ExpressionKind::Block { list, .. } = &arena[block].kind {
        assert_eq!(list.len(), width);
    } else {
        panic!("Not a block");
    }
}

#[test]
fn test_many_allocations_throughput() {
    let mut arena = IdArena::new();
    let mut builder = IrBuilder::new(&mut arena);

    let count = 100_000; // Reduced from 1,000,000 to be faster in tests
    let mut ids = Vec::with_capacity(count);

    let start = Instant::now();
    for i in 0..count {
        ids.push(builder.const_(Literal::I32(i as i32)));
    }
    let duration = start.elapsed();

    println!("Allocated {} items in {:?}", count, duration);
    // 100k allocations should be very fast with IdArena
    assert!(
        duration.as_secs_f32() < 1.0,
        "Allocation slow: {:?}",
        duration
    );
    assert_eq!(ids.len(), count);
}

#[test]
fn test_aliasing_and_mutability_safety() {
    let mut arena = IdArena::new();
    let mut builder = IrBuilder::new(&mut arena);

    let original = builder.const_(Literal::I32(42));
    let copy_id = original;

    // Mutation via alias
    if let ExpressionKind::Const(lit) = &mut arena[copy_id].kind {
        *lit = Literal::I32(100);
    }

    // Verify original reflects change
    if let ExpressionKind::Const(lit) = &arena[original].kind {
        assert_eq!(*lit, Literal::I32(100));
    }
}

#[test]
fn test_tree_construction_integrity() {
    let mut arena = IdArena::new();
    let mut builder = IrBuilder::new(&mut arena);

    // Build (i32.add (i32.const 1) (i32.const 2))
    let left = builder.const_(Literal::I32(1));
    let right = builder.const_(Literal::I32(2));
    let add_id = builder.binary(crate::ir::ops::BinaryOp::AddInt32, left, right, Type::I32);

    if let ExpressionKind::Binary {
        op,
        left: l,
        right: r,
    } = &arena[add_id].kind
    {
        assert_eq!(*op, crate::ir::ops::BinaryOp::AddInt32);

        if let ExpressionKind::Const(lit) = &arena[*l].kind {
            assert_eq!(*lit, Literal::I32(1));
        } else {
            panic!("Left not const");
        }

        if let ExpressionKind::Const(lit) = &arena[*r].kind {
            assert_eq!(*lit, Literal::I32(2));
        } else {
            panic!("Right not const");
        }
    } else {
        panic!("Not binary");
    }
}
