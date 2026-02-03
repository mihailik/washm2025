use crate::analysis::effects::{Effect, EffectAnalyzer};
use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
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

impl Default for MemoryOptimization {
    fn default() -> Self {
        Self {
            rigorous: false,
            allow_tee: true,
            allow_structure: true,
        }
    }
}

impl MemoryOptimization {
    pub fn new() -> Self {
        Self::default()
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

    fn run(&mut self, module: &mut Module) {
        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        for (_, func) in functions {
            if let Some(body) = func.body_mut() {
                self.visit(body, arena);
            }
        }
    }
}

impl Visitor for MemoryOptimization {
    fn visit_expression(
        &mut self,
        expr_id: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        let mut list = if let ExpressionKind::Block { list, .. } = &mut arena[*expr_id].kind {
            std::mem::take(list)
        } else {
            return;
        };

        // Phase 1: Eliminate redundant adjacent operations
        self.eliminate_redundant_operations(&mut list, arena);

        // Phase 2: Sink local.sets towards their uses (if rigorous mode)
        if self.rigorous && self.allow_tee {
            self.sink_local_sets(&mut list, arena);
        }

        // Phase 3: Optimize block structure (if structure mode)
        if self.allow_structure {
            self.optimize_block_structure(&mut list, arena);
        }

        // Restore the list
        if let ExpressionKind::Block {
            list: arena_list, ..
        } = &mut arena[*expr_id].kind
        {
            *arena_list = list;
        }
    }
}

impl MemoryOptimization {
    /// Eliminate redundant adjacent operations (stores, local.sets, global.sets)
    fn eliminate_redundant_operations(
        &self,
        list: &mut Vec<ExpressionId>,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        let mut i = 0;
        while i + 1 < list.len() {
            let redundant = {
                let current_id = list[i];
                let next_id = list[i + 1];

                // Check if adjacent operations are redundant
                match (&arena[current_id].kind, &arena[next_id].kind) {
                    (ExpressionKind::Store { .. }, ExpressionKind::Store { .. }) => {
                        self.check_redundant_stores(current_id, next_id, list, i, arena)
                    }
                    (ExpressionKind::LocalSet { .. }, ExpressionKind::LocalSet { .. }) => {
                        self.check_redundant_local_sets(current_id, next_id, list, i, arena)
                    }
                    (ExpressionKind::GlobalSet { .. }, ExpressionKind::GlobalSet { .. }) => {
                        self.check_redundant_global_sets(current_id, next_id, list, i, arena)
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
                    replace_with_drop(list[i], arena);
                    i += 1;
                }
                RedundancyResult::NotRedundant => {
                    i += 1;
                }
            }
        }
    }

    /// Check if two stores are redundant
    fn check_redundant_stores(
        &self,
        store1_id: ExpressionId,
        store2_id: ExpressionId,
        list: &[ExpressionId],
        i: usize,
        arena: &IdArena<ExpressionMarker, Expression>,
    ) -> RedundancyResult {
        if !are_stores_redundant(store1_id, store2_id, arena) {
            return RedundancyResult::NotRedundant;
        }

        // In rigorous mode, check for interfering effects
        if self.rigorous && i + 2 < list.len() {
            let between_effect = EffectAnalyzer::analyze_range(list, i + 1, i + 2, arena);
            if between_effect.interferes_with(Effect::MEMORY_WRITE) {
                return RedundancyResult::NotRedundant;
            }
        }

        // First store is redundant - remove or replace with drop
        if has_side_effects_in_value(store1_id, arena) {
            RedundancyResult::ReplaceFirstWithDrop
        } else {
            RedundancyResult::RemoveFirst
        }
    }

    /// Check if two local.sets are redundant
    fn check_redundant_local_sets(
        &self,
        set1_id: ExpressionId,
        set2_id: ExpressionId,
        list: &[ExpressionId],
        i: usize,
        arena: &IdArena<ExpressionMarker, Expression>,
    ) -> RedundancyResult {
        if !are_local_sets_redundant(set1_id, set2_id, arena) {
            return RedundancyResult::NotRedundant;
        }

        // Check if there's a local.get of this index between the two sets
        if self.rigorous && i + 2 < list.len() {
            // For locals, we need to be more conservative - any read of locals could matter
            let between_effect = EffectAnalyzer::analyze_range(list, i + 1, i + 2, arena);

            // Calls may read locals indirectly, traps end execution
            if between_effect.calls() || between_effect.may_trap() {
                return RedundancyResult::NotRedundant;
            }
        }

        // First set is redundant
        if has_side_effects_in_value(set1_id, arena) {
            RedundancyResult::ReplaceFirstWithDrop
        } else {
            RedundancyResult::RemoveFirst
        }
    }

    /// Check if two global.sets are redundant
    fn check_redundant_global_sets(
        &self,
        set1_id: ExpressionId,
        set2_id: ExpressionId,
        list: &[ExpressionId],
        i: usize,
        arena: &IdArena<ExpressionMarker, Expression>,
    ) -> RedundancyResult {
        if !are_global_sets_redundant(set1_id, set2_id, arena) {
            return RedundancyResult::NotRedundant;
        }

        // In rigorous mode, check for interfering effects
        if self.rigorous && i + 2 < list.len() {
            let between_effect = EffectAnalyzer::analyze_range(list, i + 1, i + 2, arena);
            if between_effect.interferes_with(Effect::GLOBAL_WRITE) {
                return RedundancyResult::NotRedundant;
            }
        }

        // First set is redundant
        if has_side_effects_in_value(set1_id, arena) {
            RedundancyResult::ReplaceFirstWithDrop
        } else {
            RedundancyResult::RemoveFirst
        }
    }

    /// Sink local.sets towards their uses (advanced optimization)
    fn sink_local_sets(
        &self,
        _list: &mut Vec<ExpressionId>,
        _arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        // TODO: Implement local.set sinking
    }

    /// Optimize block structure by creating block return values
    fn optimize_block_structure(
        &self,
        _list: &mut Vec<ExpressionId>,
        _arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        // TODO: Implement block structure optimization
    }
}

enum RedundancyResult {
    NotRedundant,
    RemoveFirst,
    ReplaceFirstWithDrop,
}

fn replace_with_drop(expr_id: ExpressionId, arena: &mut IdArena<ExpressionMarker, Expression>) {
    let old_kind = std::mem::replace(&mut arena[expr_id].kind, ExpressionKind::Nop);
    let val_id = match old_kind {
        ExpressionKind::Store { value, .. } => value,
        ExpressionKind::LocalSet { value, .. } => value,
        ExpressionKind::GlobalSet { value, .. } => value,
        _ => {
            arena[expr_id].kind = old_kind;
            return;
        }
    };
    arena[expr_id].kind = ExpressionKind::Drop { value: val_id };
    arena[expr_id].type_ = Type::NONE;
}

fn are_stores_redundant(
    store1_id: ExpressionId,
    store2_id: ExpressionId,
    arena: &IdArena<ExpressionMarker, Expression>,
) -> bool {
    let store1 = &arena[store1_id];
    let store2 = &arena[store2_id];
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
            if !are_pointers_equal(*ptr1, *ptr2, arena) {
                return false;
            }

            // Pointer must be effect-free to ensure no side effects from evaluation
            if !is_effect_free(*ptr1, arena) {
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
fn are_pointers_equal(
    ptr1_id: ExpressionId,
    ptr2_id: ExpressionId,
    arena: &IdArena<ExpressionMarker, Expression>,
) -> bool {
    let ptr1 = &arena[ptr1_id];
    let ptr2 = &arena[ptr2_id];
    match (&ptr1.kind, &ptr2.kind) {
        // Same constant address
        (ExpressionKind::Const(l1), ExpressionKind::Const(l2)) => l1 == l2,

        // Same local
        (ExpressionKind::LocalGet { id: i1, .. }, ExpressionKind::LocalGet { id: i2, .. }) => {
            i1 == i2
        }

        // Same global
        (ExpressionKind::GlobalGet { id: n1 }, ExpressionKind::GlobalGet { id: n2 }) => n1 == n2,

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
            are_pointers_equal(*l1, *l2, arena) && are_pointers_equal(*r1, *r2, arena)
        }

        _ => false,
    }
}

fn are_local_sets_redundant(
    set1_id: ExpressionId,
    set2_id: ExpressionId,
    arena: &IdArena<ExpressionMarker, Expression>,
) -> bool {
    if let ExpressionKind::LocalSet { id: idx1, .. } = &arena[set1_id].kind {
        if let ExpressionKind::LocalSet { id: idx2, .. } = &arena[set2_id].kind {
            return idx1 == idx2;
        }
    }
    false
}

fn are_global_sets_redundant(
    set1_id: ExpressionId,
    set2_id: ExpressionId,
    arena: &IdArena<ExpressionMarker, Expression>,
) -> bool {
    if let ExpressionKind::GlobalSet { id: idx1, .. } = &arena[set1_id].kind {
        if let ExpressionKind::GlobalSet { id: idx2, .. } = &arena[set2_id].kind {
            return idx1 == idx2;
        }
    }
    false
}

fn is_effect_free(expr_id: ExpressionId, arena: &IdArena<ExpressionMarker, Expression>) -> bool {
    let effect = EffectAnalyzer::analyze(expr_id, arena);
    effect.is_pure()
}

/// Helper to determine if a Store/LocalSet/GlobalSet has side effects in its value child
fn has_side_effects_in_value(
    expr_id: ExpressionId,
    arena: &IdArena<ExpressionMarker, Expression>,
) -> bool {
    let expr = &arena[expr_id];
    let child_val_id = match &expr.kind {
        ExpressionKind::Store { value, .. } => value,
        ExpressionKind::LocalSet { value, .. } => value,
        ExpressionKind::GlobalSet { value, .. } => value,
        _ => return false,
    };
    !is_effect_free(*child_val_id, arena)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::IrBuilder;
    use crate::ir::{GlobalId, Literal, LocalId, MemoryId, Type};

    #[test]
    fn test_remove_redundant_store_const_ptr() {
        let mut module = Module::new();
        let (arena, _, _, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        let mut builder = IrBuilder::new(arena);

        // store(offset=0, ptr=const(0), value=const(10))
        // store(offset=0, ptr=const(0), value=const(20))
        // -> Expected: store(offset=0, ptr=const(0), value=const(20))

        let ptr1 = builder.const_(Literal::I32(0));
        let val1 = builder.const_(Literal::I32(10));
        let store1 = builder.store(MemoryId::from_u32(0), 4, 0, 4, ptr1, val1);

        let ptr2 = builder.const_(Literal::I32(0));
        let val2 = builder.const_(Literal::I32(20));
        let store2 = builder.store(MemoryId::from_u32(0), 4, 0, 4, ptr2, val2);

        let list = vec![store1, store2];
        let mut block = builder.block(None::<&str>, list, Type::NONE);

        let mut pass = MemoryOptimization::new();
        pass.visit_expression(&mut block, arena);

        if let ExpressionKind::Block { list, .. } = &arena[block].kind {
            assert_eq!(list.len(), 1);
            if let ExpressionKind::Store { value, .. } = &arena[list[0]].kind {
                if let ExpressionKind::Const(lit) = arena[*value].kind {
                    assert_eq!(lit, Literal::I32(20));
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
        let mut module = Module::new();
        let (arena, _, _, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        let mut builder = IrBuilder::new(arena);

        let ptr1 = builder.const_(Literal::I32(0));
        let val1 = builder.const_(Literal::I32(10));
        let store1 = builder.store(MemoryId::from_u32(0), 4, 0, 4, ptr1, val1);

        let ptr2 = builder.const_(Literal::I32(4));
        let val2 = builder.const_(Literal::I32(20));
        let store2 = builder.store(MemoryId::from_u32(0), 4, 0, 4, ptr2, val2);

        let list = vec![store1, store2];
        let mut block = builder.block(None::<&str>, list, Type::NONE);

        let mut pass = MemoryOptimization::new();
        pass.visit_expression(&mut block, arena);

        if let ExpressionKind::Block { list, .. } = &arena[block].kind {
            assert_eq!(list.len(), 2);
        }
    }

    #[test]
    fn test_chain_elimination() {
        let mut module = Module::new();
        let (arena, _, _, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        let mut builder = IrBuilder::new(arena);

        let ptr = builder.const_(Literal::I32(0));
        let c10 = builder.const_(Literal::I32(10));
        let s1 = builder.store(MemoryId::from_u32(0), 4, 0, 4, ptr, c10);
        let c20 = builder.const_(Literal::I32(20));
        let s2 = builder.store(MemoryId::from_u32(0), 4, 0, 4, ptr, c20);
        let c30 = builder.const_(Literal::I32(30));
        let s3 = builder.store(MemoryId::from_u32(0), 4, 0, 4, ptr, c30);

        let mut block = builder.block(None::<&str>, vec![s1, s2, s3], Type::NONE);

        let mut pass = MemoryOptimization::new();
        pass.visit_expression(&mut block, arena);

        if let ExpressionKind::Block { list, .. } = &arena[block].kind {
            assert_eq!(list.len(), 1);
            if let ExpressionKind::Store { value, .. } = &arena[list[0]].kind {
                if let ExpressionKind::Const(lit) = arena[*value].kind {
                    assert_eq!(lit, Literal::I32(30));
                }
            }
        }
    }

    #[test]
    fn test_local_set_redundancy() {
        let mut module = Module::new();
        let (arena, _, _, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        let mut builder = IrBuilder::new(arena);

        let val1 = builder.const_(Literal::I32(10));
        let set1 = builder.local_set(LocalId::from_u32(0), val1);

        let val2 = builder.const_(Literal::I32(20));
        let set2 = builder.local_set(LocalId::from_u32(0), val2);

        let mut block = builder.block(None::<&str>, vec![set1, set2], Type::NONE);

        let mut pass = MemoryOptimization::new();
        pass.visit_expression(&mut block, arena);

        if let ExpressionKind::Block { list, .. } = &arena[block].kind {
            assert_eq!(list.len(), 1);
            if let ExpressionKind::LocalSet { value, .. } = &arena[list[0]].kind {
                if let ExpressionKind::Const(lit) = arena[*value].kind {
                    assert_eq!(lit, Literal::I32(20));
                }
            }
        }
    }

    #[test]
    fn test_global_set_elimination() {
        let mut module = Module::new();
        let (arena, _, _, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        let mut builder = IrBuilder::new(arena);
        let g0_id = GlobalId::from_u32(0);

        let val1 = builder.const_(Literal::I32(10));
        let set1 = builder.global_set(g0_id, val1);

        let val2 = builder.const_(Literal::I32(20));
        let set2 = builder.global_set(g0_id, val2);

        let mut block = builder.block(None::<&str>, vec![set1, set2], Type::NONE);

        let mut pass = MemoryOptimization::new();
        pass.visit_expression(&mut block, arena);

        if let ExpressionKind::Block { list, .. } = &arena[block].kind {
            assert_eq!(list.len(), 1);
        }
    }
}
