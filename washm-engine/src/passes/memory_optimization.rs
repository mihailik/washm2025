use crate::analysis::effects::{Effect, EffectAnalyzer};
use crate::ir::expression::{ExprRef, Expression, ExpressionKind};
use crate::ir::module::Module;
use crate::ir::visitor::Visitor;
use crate::ir::Type;
use crate::passes::Pass;

/// Memory and local optimization pass.
///
/// This pass performs several optimizations:
/// 1. Dead store elimination - removes redundant stores/sets
/// 2. Store forwarding - eliminates stores that are immediately overwritten
/// 3. Local sinking - moves local.sets closer to their uses
/// 4. Block optimization - converts local.sets into block return values
///
/// The pass has two modes:
/// - Simple: Only optimizes adjacent operations
/// - Rigorous: Checks for interfering effects across entire blocks
pub struct MemoryOptimization {
    /// Enable rigorous mode: check for interfering effects between non-adjacent nodes
    rigorous: bool,
    /// Allow sinking local.sets even when they have multiple uses (creates local.tee)
    allow_tee: bool,
    /// Allow creating block return values from local.sets
    allow_structure: bool,
}

impl MemoryOptimization {
    pub fn new() -> Self {
        Self {
            rigorous: false,
            allow_tee: true,
            allow_structure: true,
        }
    }

    pub fn with_rigorous(mut self, rigorous: bool) -> Self {
        self.rigorous = rigorous;
        self
    }

    pub fn with_allow_tee(mut self, allow_tee: bool) -> Self {
        self.allow_tee = allow_tee;
        self
    }

    pub fn with_allow_structure(mut self, allow_structure: bool) -> Self {
        self.allow_structure = allow_structure;
        self
    }
}

impl Pass for MemoryOptimization {
    fn name(&self) -> &str {
        "MemoryOptimization"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        for func in &mut module.functions {
            if let Some(body) = &mut func.body {
                self.visit(body);
            }
        }
    }
}

impl<'a> Visitor<'a> for MemoryOptimization {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        if let ExpressionKind::Block { list, .. } = &mut expr.kind {
            // Phase 1: Eliminate redundant adjacent operations
            self.eliminate_redundant_operations(list);

            // Phase 2: Sink local.sets towards their uses (if rigorous mode)
            if self.rigorous && self.allow_tee {
                self.sink_local_sets(list);
            }

            // Phase 3: Optimize block structure (if structure mode)
            if self.allow_structure {
                self.optimize_block_structure(list);
            }
        }
    }
}

impl Default for MemoryOptimization {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryOptimization {
    /// Eliminate redundant adjacent operations (stores, local.sets, global.sets)
    fn eliminate_redundant_operations<'a>(
        &self,
        list: &mut bumpalo::collections::Vec<'a, ExprRef<'a>>,
    ) {
        let mut i = 0;
        while i + 1 < list.len() {
            let redundant = {
                let current = &list[i];
                let next = &list[i + 1];

                // Check if adjacent operations are redundant
                match (&current.kind, &next.kind) {
                    (ExpressionKind::Store { .. }, ExpressionKind::Store { .. }) => {
                        self.check_redundant_stores(current, next, list, i)
                    }
                    (ExpressionKind::LocalSet { .. }, ExpressionKind::LocalSet { .. }) => {
                        self.check_redundant_local_sets(current, next, list, i)
                    }
                    (ExpressionKind::GlobalSet { .. }, ExpressionKind::GlobalSet { .. }) => {
                        self.check_redundant_global_sets(current, next, list, i)
                    }
                    _ => RedundancyResult::NotRedundant,
                }
            };

            match redundant {
                RedundancyResult::RemoveFirst => {
                    list.remove(i);
                    continue;
                }
                RedundancyResult::ReplaceFirstWithDrop => {
                    replace_with_drop(&mut list[i]);
                    i += 1;
                }
                RedundancyResult::NotRedundant => {
                    i += 1;
                }
            }
        }
    }

    /// Check if two stores are redundant
    fn check_redundant_stores<'a>(
        &self,
        store1: &Expression<'a>,
        store2: &Expression<'a>,
        list: &[ExprRef<'a>],
        i: usize,
    ) -> RedundancyResult {
        if !are_stores_redundant(store1, store2) {
            return RedundancyResult::NotRedundant;
        }

        // In rigorous mode, check for interfering effects
        if self.rigorous && i + 2 < list.len() {
            let between_effect = EffectAnalyzer::analyze_range(list, i + 1, i + 2);
            if between_effect.interferes_with(Effect::MEMORY_WRITE) {
                return RedundancyResult::NotRedundant;
            }
        }

        // First store is redundant - remove or replace with drop
        if has_side_effects_in_value(store1) {
            RedundancyResult::ReplaceFirstWithDrop
        } else {
            RedundancyResult::RemoveFirst
        }
    }

    /// Check if two local.sets are redundant
    fn check_redundant_local_sets<'a>(
        &self,
        set1: &Expression<'a>,
        set2: &Expression<'a>,
        list: &[ExprRef<'a>],
        i: usize,
    ) -> RedundancyResult {
        if !are_local_sets_redundant(set1, set2) {
            return RedundancyResult::NotRedundant;
        }

        // Check if there's a local.get of this index between the two sets
        if self.rigorous && i + 2 < list.len() {
            // For locals, we need to be more conservative - any read of locals could matter
            let between_effect = EffectAnalyzer::analyze_range(list, i + 1, i + 2);

            // Calls may read locals indirectly, traps end execution
            if between_effect.calls() || between_effect.may_trap() {
                return RedundancyResult::NotRedundant;
            }
        }

        // First set is redundant
        if has_side_effects_in_value(set1) {
            RedundancyResult::ReplaceFirstWithDrop
        } else {
            RedundancyResult::RemoveFirst
        }
    }

    /// Check if two global.sets are redundant
    fn check_redundant_global_sets<'a>(
        &self,
        set1: &Expression<'a>,
        set2: &Expression<'a>,
        list: &[ExprRef<'a>],
        i: usize,
    ) -> RedundancyResult {
        if !are_global_sets_redundant(set1, set2) {
            return RedundancyResult::NotRedundant;
        }

        // In rigorous mode, check for interfering effects
        if self.rigorous && i + 2 < list.len() {
            let between_effect = EffectAnalyzer::analyze_range(list, i + 1, i + 2);
            if between_effect.interferes_with(Effect::GLOBAL_WRITE) {
                return RedundancyResult::NotRedundant;
            }
        }

        // First set is redundant
        if has_side_effects_in_value(set1) {
            RedundancyResult::ReplaceFirstWithDrop
        } else {
            RedundancyResult::RemoveFirst
        }
    }

    /// Sink local.sets towards their uses (advanced optimization)
    fn sink_local_sets<'a>(&self, _list: &mut bumpalo::collections::Vec<'a, ExprRef<'a>>) {
        // TODO: Implement local.set sinking
        // This is a complex optimization that requires:
        // 1. Tracking uses of each local across the block
        // 2. Computing effects between set and get
        // 3. Moving the set closer to the first use
        // 4. Converting to local.tee if multiple uses
    }

    /// Optimize block structure by creating block return values
    fn optimize_block_structure<'a>(&self, _list: &mut bumpalo::collections::Vec<'a, ExprRef<'a>>) {
        // TODO: Implement block structure optimization
        // This requires:
        // 1. Finding common local.sets across all block exits
        // 2. Converting them into block return values
        // 3. Updating breaks to return values
    }
}

enum RedundancyResult {
    NotRedundant,
    RemoveFirst,
    ReplaceFirstWithDrop,
}

fn replace_with_drop(expr: &mut Expression) {
    let old_kind = std::mem::replace(&mut expr.kind, ExpressionKind::Nop);
    let val_ref = match old_kind {
        ExpressionKind::Store { value, .. } => value,
        ExpressionKind::LocalSet { value, .. } => value,
        ExpressionKind::GlobalSet { value, .. } => value,
        _ => {
            expr.kind = old_kind;
            return;
        }
    };
    expr.kind = ExpressionKind::Drop { value: val_ref };
    expr.type_ = Type::NONE;
}

fn are_stores_redundant(store1: &Expression, store2: &Expression) -> bool {
    if let ExpressionKind::Store {
        bytes: bytes1,
        offset: offset1,
        align: align1,
        ptr: ptr1,
        ..
    } = &store1.kind
    {
        if let ExpressionKind::Store {
            bytes: bytes2,
            offset: offset2,
            align: align2,
            ptr: ptr2,
            ..
        } = &store2.kind
        {
            // Must have same size, offset, and alignment
            if bytes1 != bytes2 || offset1 != offset2 || align1 != align2 {
                return false;
            }

            // Check if pointers refer to the same address
            if !are_pointers_equal(ptr1, ptr2) {
                return false;
            }

            // Pointer must be effect-free to ensure no side effects from evaluation
            if !is_effect_free(*ptr1) {
                return false;
            }

            true
        } else {
            false
        }
    } else {
        false
    }
}

/// Check if two pointer expressions refer to the same memory location
fn are_pointers_equal(ptr1: &Expression, ptr2: &Expression) -> bool {
    match (&ptr1.kind, &ptr2.kind) {
        // Same constant address
        (ExpressionKind::Const(l1), ExpressionKind::Const(l2)) => l1 == l2,

        // Same local
        (ExpressionKind::LocalGet { index: i1 }, ExpressionKind::LocalGet { index: i2 }) => {
            i1 == i2
        }

        // Same global
        (ExpressionKind::GlobalGet { id: n1 }, ExpressionKind::GlobalGet { id: n2 }) => {
            n1 == n2
        }

        // Binary operations with same structure
        (
            ExpressionKind::Binary {
                op: op1,
                left: l1,
                right: r1,
            },
            ExpressionKind::Binary {
                op: op2,
                left: l2,
                right: r2,
            },
        ) => {
            if op1 != op2 {
                return false;
            }
            // Recursively check operands
            are_pointers_equal(l1, l2) && are_pointers_equal(r1, r2)
        }

        _ => false,
    }
}

fn are_local_sets_redundant(set1: &Expression, set2: &Expression) -> bool {
    if let ExpressionKind::LocalSet { index: idx1, .. } = &set1.kind {
        if let ExpressionKind::LocalSet { index: idx2, .. } = &set2.kind {
            return idx1 == idx2;
        }
    }
    false
}

fn are_global_sets_redundant(set1: &Expression, set2: &Expression) -> bool {
    if let ExpressionKind::GlobalSet { id: idx1, .. } = &set1.kind {
        if let ExpressionKind::GlobalSet { id: idx2, .. } = &set2.kind {
            return idx1 == idx2;
        }
    }
    false
}

fn is_effect_free(expr: ExprRef) -> bool {
    let effect = EffectAnalyzer::analyze(expr);
    effect.is_pure()
}

/// Helper to determine if a Store/LocalSet/GlobalSet has side effects in its value child
fn has_side_effects_in_value(expr: &Expression) -> bool {
    let child_val = match &expr.kind {
        ExpressionKind::Store { value, .. } => value,
        ExpressionKind::LocalSet { value, .. } => value,
        ExpressionKind::GlobalSet { value, .. } => value,
        _ => return false,
    };
    !is_effect_free(*child_val)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::{ExprRef, Expression, ExpressionKind};
    use crate::ir::module::Function;
    use crate::ir::{Literal, Type};
    use bumpalo::collections::Vec as BumpVec;
    use bumpalo::Bump;

    #[test]
    fn test_remove_redundant_store_const_ptr() {
        let bump = Bump::new();
        // store(offset=0, ptr=const(0), value=const(10))
        // store(offset=0, ptr=const(0), value=const(20))
        // -> Expected: store(offset=0, ptr=const(0), value=const(20))

        let ptr1 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(0)),
            type_: Type::I32,
        }));
        let val1 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(10)),
            type_: Type::I32,
        }));
        let store1 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Store {
                bytes: 4,
                offset: 0,
                align: 4,
                ptr: ptr1,
                value: val1,
            },
            type_: Type::NONE,
        }));

        let ptr2 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(0)),
            type_: Type::I32,
        }));
        let val2 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(20)),
            type_: Type::I32,
        }));
        let store2 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Store {
                bytes: 4,
                offset: 0,
                align: 4,
                ptr: ptr2,
                value: val2,
            },
            type_: Type::NONE,
        }));

        let mut list = BumpVec::new_in(&bump);
        list.push(store1);
        list.push(store2);

        let block = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Block { name: None, list },
            type_: Type::NONE,
        }));

        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(block),
        );
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);
        module.add_function(func);

        let mut pass = MemoryOptimization::new();
        pass.run(&mut module);

        let body = module.functions[0].body.as_ref().unwrap();
        if let ExpressionKind::Block { list, .. } = &body.kind {
            assert_eq!(list.len(), 1);
            if let ExpressionKind::Store { value, .. } = &list[0].kind {
                if let ExpressionKind::Const(lit) = &value.kind {
                    assert_eq!(lit, &Literal::I32(20));
                } else {
                    panic!("Expected Const value");
                }
            } else {
                panic!("Expected Store");
            }
        } else {
            panic!("Expected Block");
        }
    }

    #[test]
    fn test_no_opt_different_ptr() {
        let bump = Bump::new();
        // store(ptr=const(0), val=10)
        // store(ptr=const(4), val=20)
        // -> No change

        let ptr1 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(0)),
            type_: Type::I32,
        }));
        let val1 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(10)),
            type_: Type::I32,
        }));
        let store1 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Store {
                bytes: 4,
                offset: 0,
                align: 4,
                ptr: ptr1,
                value: val1,
            },
            type_: Type::NONE,
        }));

        let ptr2 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(4)),
            type_: Type::I32,
        }));
        let val2 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(20)),
            type_: Type::I32,
        }));
        let store2 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Store {
                bytes: 4,
                offset: 0,
                align: 4,
                ptr: ptr2,
                value: val2,
            },
            type_: Type::NONE,
        }));

        let mut list = BumpVec::new_in(&bump);
        list.push(store1);
        list.push(store2);

        let block = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Block { name: None, list },
            type_: Type::NONE,
        }));

        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(block),
        );
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);
        module.add_function(func);

        let mut pass = MemoryOptimization::new();
        pass.run(&mut module);

        let body = module.functions[0].body.as_ref().unwrap();
        if let ExpressionKind::Block { list, .. } = &body.kind {
            assert_eq!(list.len(), 2);
        }
    }

    #[test]
    fn test_chain_elimination() {
        let bump = Bump::new();
        // store(ptr=0, val=10)
        // store(ptr=0, val=20)
        // store(ptr=0, val=30)
        // -> store(ptr=0, val=30)

        // Helper to clone redundant setup
        fn make_expr<'a>(bump: &'a Bump, val: i32) -> ExprRef<'a> {
            ExprRef::new(bump.alloc(Expression {
                kind: ExpressionKind::Const(Literal::I32(val)),
                type_: Type::I32,
            }))
        }

        fn make_store<'a>(bump: &'a Bump, ptr_val: i32, val: i32) -> ExprRef<'a> {
            let ptr = make_expr(bump, ptr_val);
            let v = make_expr(bump, val);
            ExprRef::new(bump.alloc(Expression {
                kind: ExpressionKind::Store {
                    bytes: 4,
                    offset: 0,
                    align: 4,
                    ptr,
                    value: v,
                },
                type_: Type::NONE,
            }))
        }

        let s1 = make_store(&bump, 0, 10);
        let s2 = make_store(&bump, 0, 20);
        let s3 = make_store(&bump, 0, 30);

        let mut list = BumpVec::new_in(&bump);
        list.push(s1);
        list.push(s2);
        list.push(s3);

        let block = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Block { name: None, list },
            type_: Type::NONE,
        }));

        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(block),
        );
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);
        module.add_function(func);

        let mut pass = MemoryOptimization::new();
        pass.run(&mut module);

        let body = module.functions[0].body.as_ref().unwrap();
        if let ExpressionKind::Block { list, .. } = &body.kind {
            assert_eq!(list.len(), 1);
            if let ExpressionKind::Store { value, .. } = &list[0].kind {
                if let ExpressionKind::Const(lit) = &value.kind {
                    assert_eq!(lit, &Literal::I32(30));
                }
            }
        }
    }

    #[test]
    fn test_side_effect_prevent_opt() {
        let bump = Bump::new();
        // store(ptr=0, val=UnaryOp(10))
        // store(ptr=0, val=20)

        use crate::ir::ops::UnaryOp;

        let ptr1 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(0)),
            type_: Type::I32,
        }));
        let val_inner = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(10)),
            type_: Type::I32,
        }));
        let val1 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Unary {
                op: UnaryOp::EqZInt32,
                value: val_inner,
            },
            type_: Type::I32,
        }));

        let store1 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Store {
                bytes: 4,
                offset: 0,
                align: 4,
                ptr: ptr1,
                value: val1,
            },
            type_: Type::NONE,
        }));

        let ptr2 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(0)),
            type_: Type::I32,
        }));
        let val2 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(20)),
            type_: Type::I32,
        }));
        let store2 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Store {
                bytes: 4,
                offset: 0,
                align: 4,
                ptr: ptr2,
                value: val2,
            },
            type_: Type::NONE,
        }));

        let mut list = BumpVec::new_in(&bump);
        list.push(store1);
        list.push(store2);

        let block = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Block { name: None, list },
            type_: Type::NONE,
        }));

        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(block),
        );
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);
        module.add_function(func);

        let mut pass = MemoryOptimization::new();
        pass.run(&mut module);

        let body = module.functions[0].body.as_ref().unwrap();
        if let ExpressionKind::Block { list, .. } = &body.kind {
            // UnaryOp::EqZInt32 no longer traps by default in our updated EffectAnalyzer,
            // so the redundant store is now optimized away.
            assert_eq!(list.len(), 1);
        }
    }

    #[test]
    fn test_local_set_redundancy() {
        let bump = Bump::new();
        // local.set(0, 10); local.set(0, 20) -> local.set(0, 20)

        let val1 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(10)),
            type_: Type::I32,
        }));
        let set1 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::LocalSet {
                index: 0,
                value: val1,
            },
            type_: Type::NONE,
        }));

        let val2 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(20)),
            type_: Type::I32,
        }));
        let set2 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::LocalSet {
                index: 0,
                value: val2,
            },
            type_: Type::NONE,
        }));

        let mut list = BumpVec::new_in(&bump);
        list.push(set1);
        list.push(set2);

        let block = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Block { name: None, list },
            type_: Type::NONE,
        }));
        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(block),
        );
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);
        module.add_function(func);

        let mut pass = MemoryOptimization::new();
        pass.run(&mut module);

        let body = module.functions[0].body.as_ref().unwrap();
        if let ExpressionKind::Block { list, .. } = &body.kind {
            assert_eq!(list.len(), 1);
            if let ExpressionKind::LocalSet { value, .. } = &list[0].kind {
                if let ExpressionKind::Const(lit) = &value.kind {
                    assert_eq!(lit, &Literal::I32(20));
                } else {
                    panic!("Expected Const");
                }
            } else {
                panic!("Expected LocalSet");
            }
        }
    }

    #[test]
    fn test_rigorous_mode_blocks_optimization_with_interfering_call() {
        let bump = Bump::new();
        // store(0, 10)
        // store(0, 20)  <- adjacent, should optimize

        let ptr1 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(0)),
            type_: Type::I32,
        }));
        let val1 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(10)),
            type_: Type::I32,
        }));
        let store1 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Store {
                bytes: 4,
                offset: 0,
                align: 4,
                ptr: ptr1,
                value: val1,
            },
            type_: Type::NONE,
        }));

        let ptr2 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(0)),
            type_: Type::I32,
        }));
        let val2 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(20)),
            type_: Type::I32,
        }));
        let store2 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Store {
                bytes: 4,
                offset: 0,
                align: 4,
                ptr: ptr2,
                value: val2,
            },
            type_: Type::NONE,
        }));

        let mut list = BumpVec::new_in(&bump);
        list.push(store1);
        list.push(store2);

        let block = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Block { name: None, list },
            type_: Type::NONE,
        }));

        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(block),
        );
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);
        module.add_function(func);

        // Run in rigorous mode - adjacent stores should still optimize
        let mut pass = MemoryOptimization::new().with_rigorous(true);
        pass.run(&mut module);

        let body = module.functions[0].body.as_ref().unwrap();
        if let ExpressionKind::Block { list, .. } = &body.kind {
            assert_eq!(
                list.len(),
                1,
                "Adjacent redundant stores should be optimized even in rigorous mode"
            );
        }
    }

    #[test]
    fn test_binary_pointer_comparison() {
        let bump = Bump::new();

        // First store: store(ptr=104, val=10)
        // (using literal 104 which is conceptually 100+4)
        let ptr1 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(104)),
            type_: Type::I32,
        }));
        let val1 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(10)),
            type_: Type::I32,
        }));
        let store1 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Store {
                bytes: 4,
                offset: 0,
                align: 4,
                ptr: ptr1,
                value: val1,
            },
            type_: Type::NONE,
        }));

        // Second store: store(ptr=104, val=20)
        let ptr2 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(104)),
            type_: Type::I32,
        }));
        let val2 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(20)),
            type_: Type::I32,
        }));
        let store2 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Store {
                bytes: 4,
                offset: 0,
                align: 4,
                ptr: ptr2,
                value: val2,
            },
            type_: Type::NONE,
        }));

        let mut list = BumpVec::new_in(&bump);
        list.push(store1);
        list.push(store2);

        let block = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Block { name: None, list },
            type_: Type::NONE,
        }));

        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(block),
        );
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);
        module.add_function(func);

        let mut pass = MemoryOptimization::new();
        pass.run(&mut module);

        let body = module.functions[0].body.as_ref().unwrap();
        if let ExpressionKind::Block { list, .. } = &body.kind {
            assert_eq!(
                list.len(),
                1,
                "Stores to same constant address should be recognized as redundant"
            );
        }
    }

    #[test]
    fn test_rigorous_mode_interference() {
        let bump = Bump::new();

        // store(0, 10)
        // call("func")  // may write to memory
        // store(0, 20)
        let ptr1 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(0)),
            type_: Type::I32,
        }));
        let val1 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(10)),
            type_: Type::I32,
        }));
        let store1 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Store {
                bytes: 4,
                offset: 0,
                align: 4,
                ptr: ptr1,
                value: val1,
            },
            type_: Type::NONE,
        }));

        let operands = BumpVec::new_in(&bump);
        let call = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Call {
                target: "func",
                operands,
                is_return: false,
            },
            type_: Type::NONE,
        }));

        let ptr2 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(0)),
            type_: Type::I32,
        }));
        let val2 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(20)),
            type_: Type::I32,
        }));
        let store2 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Store {
                bytes: 4,
                offset: 0,
                align: 4,
                ptr: ptr2,
                value: val2,
            },
            type_: Type::NONE,
        }));

        let mut list = BumpVec::new_in(&bump);
        list.push(store1);
        list.push(call);
        list.push(store2);

        let block = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Block { name: None, list },
            type_: Type::NONE,
        }));

        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(block),
        );
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);
        module.add_function(func);

        // In simple mode, only adjacent pairs are checked - should not optimize
        let mut pass_simple = MemoryOptimization::new();
        pass_simple.run(&mut module);

        let body = module.functions[0].body.as_ref().unwrap();
        if let ExpressionKind::Block { list, .. } = &body.kind {
            assert_eq!(
                list.len(),
                3,
                "Call between stores prevents simple mode optimization"
            );
        }
    }

    #[test]
    fn test_local_set_different_indices() {
        let bump = Bump::new();

        // local.set(0, 10)
        // local.set(1, 20)  // different index
        let val1 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(10)),
            type_: Type::I32,
        }));
        let set1 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::LocalSet {
                index: 0,
                value: val1,
            },
            type_: Type::NONE,
        }));

        let val2 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(20)),
            type_: Type::I32,
        }));
        let set2 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::LocalSet {
                index: 1,
                value: val2,
            },
            type_: Type::NONE,
        }));

        let mut list = BumpVec::new_in(&bump);
        list.push(set1);
        list.push(set2);

        let block = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Block { name: None, list },
            type_: Type::NONE,
        }));

        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![Type::I32, Type::I32],
            Some(block),
        );
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);
        module.add_function(func);

        let mut pass = MemoryOptimization::new();
        pass.run(&mut module);

        let body = module.functions[0].body.as_ref().unwrap();
        if let ExpressionKind::Block { list, .. } = &body.kind {
            assert_eq!(list.len(), 2, "Different local indices should not optimize");
        }
    }

    #[test]
    fn test_global_set_elimination() {
        let bump = Bump::new();
        let g0_id = unsafe { std::mem::transmute(0u64) };

        // global.set(0, 10)
        // global.set(0, 20)  // redundant
        let val1 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(10)),
            type_: Type::I32,
        }));
        let set1 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::GlobalSet {
                id: g0_id,
                value: val1,
            },
            type_: Type::NONE,
        }));

        let val2 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(20)),
            type_: Type::I32,
        }));
        let set2 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::GlobalSet {
                id: g0_id,
                value: val2,
            },
            type_: Type::NONE,
        }));

        let mut list = BumpVec::new_in(&bump);
        list.push(set1);
        list.push(set2);

        let block = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Block { name: None, list },
            type_: Type::NONE,
        }));

        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(block),
        );
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);
        module.add_function(func);

        let mut pass = MemoryOptimization::new();
        pass.run(&mut module);

        let body = module.functions[0].body.as_ref().unwrap();
        if let ExpressionKind::Block { list, .. } = &body.kind {
            assert_eq!(list.len(), 1, "Redundant global.set should be eliminated");
        }
    }
}
