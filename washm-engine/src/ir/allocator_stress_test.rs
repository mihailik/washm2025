use crate::ir::expression::{ExpressionKind, IrBuilder};
use crate::ir::{Literal, Type};
use bumpalo::Bump;
use std::time::Instant;

#[test]
fn test_deep_recursion_stress() {
    let bump = Bump::new();
    let builder = IrBuilder::new(&bump);

    // Testing stack depth / arena behavior with deep chain
    // Bumpalo creates a linked list of chunks.
    let depth = 50_000;
    let mut expr = builder.const_(Literal::I32(0));

    for _ in 0..depth {
        expr = builder.unary(crate::ir::ops::UnaryOp::EqZInt32, expr, Type::I32);
    }

    assert_eq!(expr.type_, Type::I32);
}

#[test]
fn test_wide_allocation_stress() {
    let bump = Bump::new();
    let builder = IrBuilder::new(&bump);

    let width = 100_000;
    // We use bumpalo Vec directly to interact with Arena-allocated lists
    let mut list = bumpalo::collections::Vec::with_capacity_in(width, &bump);

    for i in 0..width {
        list.push(builder.const_(Literal::I32(i as i32)));
    }

    let block = builder.block(None, list, Type::NONE);

    if let ExpressionKind::Block { list, .. } = &block.kind {
        assert_eq!(list.len(), width);
    } else {
        panic!("Not a block");
    }
}

#[test]
fn test_many_allocations_throughput() {
    let bump = Bump::new();
    let builder = IrBuilder::new(&bump);

    let count = 1_000_000;
    let mut refs = Vec::with_capacity(count);

    let start = Instant::now();
    for i in 0..count {
        refs.push(builder.const_(Literal::I32(i as i32)));
    }
    let duration = start.elapsed();

    println!("Allocated {} items in {:?}", count, duration);
    // 1 million allocations should be very fast with Bumpalo (usually sub-second)
    // We put a generous 5s limit to avoid flakes on slow CI/debug builds
    assert!(
        duration.as_secs_f32() < 5.0,
        "Allocation slow: {:?}",
        duration
    );
    assert_eq!(refs.len(), count);
}

#[test]
fn test_aliasing_and_mutability_safety() {
    let bump = Bump::new();
    let builder = IrBuilder::new(&bump);

    let original = builder.const_(Literal::I32(42));
    let mut copy_ref = original;

    // Mutation via alias
    // We access the mutable reference provided by DerefMut on ExprRef
    // Since copy_ref points to the same underlying memory as original,
    // modification should be visible.

    if let ExpressionKind::Const(lit) = &mut copy_ref.kind {
        *lit = Literal::I32(100);
    }

    // Verify original reflects change
    if let ExpressionKind::Const(lit) = &original.kind {
        assert_eq!(*lit, Literal::I32(100));
    }
}

#[test]
fn test_tree_construction_integrity() {
    let bump = Bump::new();
    let builder = IrBuilder::new(&bump);

    // Build (i32.add (i32.const 1) (i32.const 2))
    let left = builder.const_(Literal::I32(1));
    let right = builder.const_(Literal::I32(2));
    let add = builder.binary(crate::ir::ops::BinaryOp::AddInt32, left, right, Type::I32);

    if let ExpressionKind::Binary {
        op,
        left: l,
        right: r,
    } = &add.kind
    {
        assert_eq!(*op, crate::ir::ops::BinaryOp::AddInt32);

        if let ExpressionKind::Const(lit) = &l.kind {
            assert_eq!(*lit, Literal::I32(1));
        } else {
            panic!("Left not const");
        }

        if let ExpressionKind::Const(lit) = &r.kind {
            assert_eq!(*lit, Literal::I32(2));
        } else {
            panic!("Right not const");
        }
    } else {
        panic!("Not binary");
    }
}
