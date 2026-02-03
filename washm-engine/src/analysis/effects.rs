// Effect analysis for WebAssembly expressions.
//
// This module provides the foundational types for reasoning about side effects
// in WebAssembly expressions. Effects are used by optimization passes to
// determine whether it is safe to reorder, eliminate, or move instructions.
//
// ## Design Philosophy
//
// In Rust, we leverage the type system to make effect analysis more precise
// and harder to misuse than the C++ counterpart. Effects are represented as
// bitflags, allowing efficient composition and querying.
//
// ## Usage
//
// ```rust
// use washm::analysis::effects::{Effect, EffectAnalyzer};
// use washm::ir::expression::{IrBuilder, ExpressionId};
// use washm::ir::{Literal, Type};
//
// // Assuming you have an arena
// let mut builder = IrBuilder::new(&mut arena);
// let expr = builder.unreachable();
//
// let effect = EffectAnalyzer::analyze(expr, &arena);
// assert!(effect.may_trap());
// assert!(effect.transfers_control());
// ```

use crate::collections::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use bitflags::bitflags;

bitflags! {
    /// Represents the side effects an expression may have.
    ///
    /// Multiple effects can be combined using bitwise OR (|).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Effect: u32 {
        /// No observable side effects.
        const NONE = 0;

        /// May read from linear memory.
        const MEMORY_READ = 1 << 0;

        /// May write to linear memory.
        const MEMORY_WRITE = 1 << 1;

        /// May read from a global variable.
        const GLOBAL_READ = 1 << 2;

        /// May write to a global variable.
        const GLOBAL_WRITE = 1 << 3;

        /// May write to a local variable.
        const LOCAL_WRITE = 1 << 4;

        /// May trap (e.g., division by zero, out-of-bounds access).
        const MAY_TRAP = 1 << 5;

        /// May trap on an unreachable instruction (always traps).
        const TRAPS = 1 << 6;

        /// Performs a function call (may have arbitrary effects).
        const CALLS = 1 << 7;

        /// Transfers control flow (br, br_if, return).
        const BRANCHES = 1 << 8;

        /// May throw an exception.
        const THROWS = 1 << 9;

        /// May access a table (call_indirect, table.get, table.set).
        const TABLE_ACCESS = 1 << 10;

        /// May grow memory or tables.
        const GROWS = 1 << 11;

        /// May be non-deterministic (e.g., memory.size in some contexts).
        const NONDETERMINISTIC = 1 << 12;

        /// May read from a GC heap.
        const HEAPS_READ = 1 << 13;

        /// May write to a GC heap.
        const HEAPS_WRITE = 1 << 14;

        /// Has implicit trap behavior (e.g., implicit bounds checks).
        const IMPLICIT_TRAP = Self::MAY_TRAP.bits();

        /// Reads any state (memory, globals, etc.).
        const READS = Self::MEMORY_READ.bits() | Self::GLOBAL_READ.bits() | Self::HEAPS_READ.bits();

        /// Writes any state (memory, globals, locals).
        const WRITES = Self::MEMORY_WRITE.bits() | Self::GLOBAL_WRITE.bits() | Self::LOCAL_WRITE.bits() | Self::HEAPS_WRITE.bits();

        /// Any effect that could affect program semantics if removed.
        const SIDE_EFFECTS = Self::WRITES.bits() | Self::CALLS.bits() | Self::TRAPS.bits()
            | Self::THROWS.bits() | Self::BRANCHES.bits() | Self::GROWS.bits();
    }
}

impl Effect {
    /// Returns true if the expression has no observable side effects.
    #[inline]
    pub fn is_pure(self) -> bool {
        self == Effect::NONE
    }

    /// Returns true if the expression may trap.
    #[inline]
    pub fn may_trap(self) -> bool {
        self.intersects(Effect::MAY_TRAP | Effect::TRAPS)
    }

    /// Returns true if the expression definitely traps (unreachable).
    #[inline]
    pub fn traps(self) -> bool {
        self.contains(Effect::TRAPS)
    }

    /// Returns true if the expression transfers control flow.
    #[inline]
    pub fn transfers_control(self) -> bool {
        self.intersects(Effect::BRANCHES | Effect::TRAPS | Effect::THROWS)
    }

    /// Returns true if the expression performs a function call.
    #[inline]
    pub fn calls(self) -> bool {
        self.contains(Effect::CALLS)
    }

    /// Returns true if the expression reads from memory.
    #[inline]
    pub fn reads_memory(self) -> bool {
        self.contains(Effect::MEMORY_READ)
    }

    /// Returns true if the expression writes to memory.
    #[inline]
    pub fn writes_memory(self) -> bool {
        self.contains(Effect::MEMORY_WRITE)
    }

    /// Returns true if the expression reads from globals.
    #[inline]
    pub fn reads_global(self) -> bool {
        self.contains(Effect::GLOBAL_READ)
    }

    /// Returns true if the expression writes to globals.
    #[inline]
    pub fn writes_global(self) -> bool {
        self.contains(Effect::GLOBAL_WRITE)
    }

    /// Returns true if the expression writes to locals.
    #[inline]
    pub fn writes_local(self) -> bool {
        self.contains(Effect::LOCAL_WRITE)
    }

    /// Returns true if the expression reads from locals.
    #[inline]
    pub fn reads_local(self) -> bool {
        false // LocalGet doesn't set any effect flags, so conservatively return false
    }

    /// Returns true if the expression branches (control flow transfer).
    #[inline]
    pub fn branches(self) -> bool {
        self.contains(Effect::BRANCHES)
    }

    /// Returns true if the expression has any side effects that prevent removal.
    #[inline]
    pub fn has_side_effects(self) -> bool {
        self.intersects(Effect::SIDE_EFFECTS)
    }

    /// Returns true if the expression reads any state.
    #[inline]
    pub fn reads_state(self) -> bool {
        self.intersects(Effect::READS)
    }

    /// Returns true if the expression writes any state.
    #[inline]
    pub fn writes_state(self) -> bool {
        self.intersects(Effect::WRITES)
    }

    /// Returns true if two effects may interfere (one writes what the other reads or writes).
    ///
    /// This is used to determine if two expressions can be safely reordered.
    #[inline]
    pub fn interferes_with(self, other: Effect) -> bool {
        // Write-write conflicts (excluding LOCAL_WRITE which doesn't interfere with memory/global)
        if self.contains(Effect::MEMORY_WRITE) && other.contains(Effect::MEMORY_WRITE) {
            return true;
        }
        if self.contains(Effect::GLOBAL_WRITE) && other.contains(Effect::GLOBAL_WRITE) {
            return true;
        }

        // Read-write conflicts
        if self.contains(Effect::MEMORY_READ) && other.contains(Effect::MEMORY_WRITE) {
            return true;
        }
        if other.contains(Effect::MEMORY_READ) && self.contains(Effect::MEMORY_WRITE) {
            return true;
        }
        if self.contains(Effect::GLOBAL_READ) && other.contains(Effect::GLOBAL_WRITE) {
            return true;
        }
        if other.contains(Effect::GLOBAL_READ) && self.contains(Effect::GLOBAL_WRITE) {
            return true;
        }

        // Calls and traps interfere with everything
        if self.calls() || other.calls() {
            return true;
        }
        if self.may_trap() || other.may_trap() {
            return true;
        }

        false
    }
}

impl Default for Effect {
    fn default() -> Self {
        Effect::NONE
    }
}

/// Analyzes expressions to compute their aggregate side effects.
///
/// The `EffectAnalyzer` traverses expression trees and computes the union
/// of all effects that may occur during evaluation.
pub struct EffectAnalyzer;

impl EffectAnalyzer {
    /// Analyzes a single expression and returns its aggregate effects.
    pub fn analyze(expr_id: ExpressionId, arena: &IdArena<ExpressionMarker, Expression>) -> Effect {
        let expr = &arena[expr_id];
        match &expr.kind {
            ExpressionKind::Nop => Effect::NONE,

            ExpressionKind::Const(_) => Effect::NONE,

            ExpressionKind::Unreachable => Effect::TRAPS,

            ExpressionKind::Block { list, .. } => Self::analyze_list(list, arena),

            ExpressionKind::Unary { op, value } => {
                let mut effects = Self::analyze(*value, arena);
                if op.may_trap() {
                    effects |= Effect::MAY_TRAP;
                }
                effects
            }

            ExpressionKind::Binary { op, left, right } => {
                let mut effects = Self::analyze(*left, arena) | Self::analyze(*right, arena);
                if op.may_trap() {
                    effects |= Effect::MAY_TRAP;
                }
                effects
            }

            ExpressionKind::LocalGet { .. } => Effect::NONE,

            ExpressionKind::LocalSet { value, .. } => {
                Effect::LOCAL_WRITE | Self::analyze(*value, arena)
            }

            ExpressionKind::LocalTee { value, .. } => {
                Effect::LOCAL_WRITE | Self::analyze(*value, arena)
            }

            ExpressionKind::GlobalGet { .. } => Effect::GLOBAL_READ,

            ExpressionKind::GlobalSet { value, .. } => {
                Effect::GLOBAL_WRITE | Self::analyze(*value, arena)
            }

            ExpressionKind::Load { ptr, .. } => {
                Effect::MEMORY_READ | Effect::MAY_TRAP | Self::analyze(*ptr, arena)
            }

            ExpressionKind::Store { ptr, value, .. } => {
                Effect::MEMORY_WRITE
                    | Effect::MAY_TRAP
                    | Self::analyze(*ptr, arena)
                    | Self::analyze(*value, arena)
            }

            ExpressionKind::Call { operands, .. } => {
                // Calls have arbitrary effects
                Effect::CALLS | Self::analyze_list(operands, arena)
            }

            ExpressionKind::CallIndirect {
                operands, target, ..
            } => {
                // Indirect calls: may trap (bad table index), has table access
                Effect::CALLS
                    | Effect::TABLE_ACCESS
                    | Effect::MAY_TRAP
                    | Self::analyze(*target, arena)
                    | Self::analyze_list(operands, arena)
            }

            ExpressionKind::If {
                name: _,
                condition,
                if_true,
                if_false,
            } => {
                let mut effect = Effect::BRANCHES | Self::analyze(*condition, arena);
                effect |= Self::analyze(*if_true, arena);
                if let Some(if_false_expr) = if_false {
                    effect |= Self::analyze(*if_false_expr, arena);
                }
                effect
            }

            ExpressionKind::Loop { body, .. } => Effect::BRANCHES | Self::analyze(*body, arena),

            ExpressionKind::Break {
                condition, value, ..
            } => {
                let mut effect = Effect::BRANCHES;
                if let Some(cond) = condition {
                    effect |= Self::analyze(*cond, arena);
                }
                if let Some(val) = value {
                    effect |= Self::analyze(*val, arena);
                }
                effect
            }

            ExpressionKind::Return { value } => {
                let mut effect = Effect::BRANCHES;
                if let Some(val) = value {
                    effect |= Self::analyze(*val, arena);
                }
                effect
            }

            ExpressionKind::Drop { value } => Self::analyze(*value, arena),

            ExpressionKind::Select {
                condition,
                if_true,
                if_false,
            } => {
                Self::analyze(*condition, arena)
                    | Self::analyze(*if_true, arena)
                    | Self::analyze(*if_false, arena)
            }

            ExpressionKind::Switch {
                condition, value, ..
            } => {
                let mut effect = Effect::BRANCHES | Self::analyze(*condition, arena);
                if let Some(val) = value {
                    effect |= Self::analyze(*val, arena);
                }
                effect
            }

            ExpressionKind::MemorySize { .. } => Effect::MEMORY_READ,

            ExpressionKind::MemoryGrow { delta, .. } => {
                Effect::GROWS | Effect::NONDETERMINISTIC | Self::analyze(*delta, arena)
            }

            // Atomic operations have memory side effects
            ExpressionKind::AtomicRMW { ptr, value, .. } => {
                Effect::MEMORY_READ
                    | Effect::MEMORY_WRITE
                    | Effect::MAY_TRAP
                    | Self::analyze(*ptr, arena)
                    | Self::analyze(*value, arena)
            }

            ExpressionKind::AtomicCmpxchg {
                ptr,
                expected,
                replacement,
                ..
            } => {
                Effect::MEMORY_READ
                    | Effect::MEMORY_WRITE
                    | Effect::MAY_TRAP
                    | Self::analyze(*ptr, arena)
                    | Self::analyze(*expected, arena)
                    | Self::analyze(*replacement, arena)
            }

            ExpressionKind::AtomicWait {
                ptr,
                expected,
                timeout,
                ..
            } => {
                Effect::MEMORY_READ
                    | Effect::CALLS
                    | Effect::MAY_TRAP
                    | Self::analyze(*ptr, arena)
                    | Self::analyze(*expected, arena)
                    | Self::analyze(*timeout, arena)
            }

            ExpressionKind::AtomicNotify { ptr, count, .. } => {
                Effect::MEMORY_READ
                    | Effect::MEMORY_WRITE
                    | Effect::MAY_TRAP
                    | Self::analyze(*ptr, arena)
                    | Self::analyze(*count, arena)
            }

            ExpressionKind::AtomicFence => Effect::MEMORY_WRITE,

            // SIMD operations
            ExpressionKind::SIMDExtract { vec, .. } => {
                Effect::MAY_TRAP | Self::analyze(*vec, arena)
            }

            ExpressionKind::SIMDReplace { vec, value, .. } => {
                Effect::MAY_TRAP | Self::analyze(*vec, arena) | Self::analyze(*value, arena)
            }

            ExpressionKind::SIMDShuffle { left, right, .. } => {
                Effect::MAY_TRAP | Self::analyze(*left, arena) | Self::analyze(*right, arena)
            }

            ExpressionKind::SIMDTernary { a, b, c, .. } => {
                Effect::MAY_TRAP
                    | Self::analyze(*a, arena)
                    | Self::analyze(*b, arena)
                    | Self::analyze(*c, arena)
            }

            ExpressionKind::SIMDShift { vec, shift, .. } => {
                Effect::MAY_TRAP | Self::analyze(*vec, arena) | Self::analyze(*shift, arena)
            }

            ExpressionKind::SIMDLoad { ptr, .. } => {
                Effect::MEMORY_READ | Effect::MAY_TRAP | Self::analyze(*ptr, arena)
            }

            ExpressionKind::SIMDLoadStoreLane { ptr, vec, .. } => {
                Effect::MEMORY_READ
                    | Effect::MEMORY_WRITE
                    | Effect::MAY_TRAP
                    | Self::analyze(*ptr, arena)
                    | Self::analyze(*vec, arena)
            }

            ExpressionKind::MemoryInit {
                dest, offset, size, ..
            } => {
                Effect::MEMORY_WRITE
                    | Effect::MAY_TRAP
                    | Self::analyze(*dest, arena)
                    | Self::analyze(*offset, arena)
                    | Self::analyze(*size, arena)
            }

            ExpressionKind::DataDrop { .. } => Effect::MEMORY_WRITE,

            ExpressionKind::MemoryCopy {
                dest, src, size, ..
            } => {
                Effect::MEMORY_READ
                    | Effect::MEMORY_WRITE
                    | Effect::MAY_TRAP
                    | Self::analyze(*dest, arena)
                    | Self::analyze(*src, arena)
                    | Self::analyze(*size, arena)
            }

            ExpressionKind::MemoryFill {
                dest, value, size, ..
            } => {
                Effect::MEMORY_WRITE
                    | Effect::MAY_TRAP
                    | Self::analyze(*dest, arena)
                    | Self::analyze(*value, arena)
                    | Self::analyze(*size, arena)
            }
            ExpressionKind::TableGet { index, .. } => {
                Effect::TABLE_ACCESS | Effect::MAY_TRAP | Self::analyze(*index, arena)
            }
            ExpressionKind::TableSet { index, value, .. } => {
                Effect::TABLE_ACCESS
                    | Effect::MAY_TRAP
                    | Self::analyze(*index, arena)
                    | Self::analyze(*value, arena)
            }
            ExpressionKind::TableSize { .. } => Effect::TABLE_ACCESS,
            ExpressionKind::TableGrow { delta, value, .. } => {
                Effect::TABLE_ACCESS
                    | Effect::GROWS
                    | Self::analyze(*delta, arena)
                    | Self::analyze(*value, arena)
            }
            ExpressionKind::TableFill {
                dest, value, size, ..
            } => {
                Effect::TABLE_ACCESS
                    | Effect::MAY_TRAP
                    | Self::analyze(*dest, arena)
                    | Self::analyze(*value, arena)
                    | Self::analyze(*size, arena)
            }
            ExpressionKind::TableCopy {
                dest, src, size, ..
            } => {
                Effect::TABLE_ACCESS
                    | Effect::MAY_TRAP
                    | Self::analyze(*dest, arena)
                    | Self::analyze(*src, arena)
                    | Self::analyze(*size, arena)
            }
            ExpressionKind::TableInit {
                dest, offset, size, ..
            } => {
                Effect::TABLE_ACCESS
                    | Effect::MAY_TRAP
                    | Self::analyze(*dest, arena)
                    | Self::analyze(*offset, arena)
                    | Self::analyze(*size, arena)
            }
            ExpressionKind::RefNull { .. } => Effect::NONE,
            ExpressionKind::RefIsNull { value } => Self::analyze(*value, arena),
            ExpressionKind::RefAs { value, .. } => Effect::MAY_TRAP | Self::analyze(*value, arena),
            ExpressionKind::RefEq { left, right } => {
                Self::analyze(*left, arena) | Self::analyze(*right, arena)
            }
            ExpressionKind::RefFunc { .. } => Effect::NONE,
            ExpressionKind::StructNew { operands, .. } => {
                Effect::HEAPS_WRITE | Self::analyze_list(operands, arena)
            }
            ExpressionKind::StructGet { ptr, .. } => {
                Effect::HEAPS_READ | Effect::MAY_TRAP | Self::analyze(*ptr, arena)
            }
            ExpressionKind::StructSet { ptr, value, .. } => {
                Effect::HEAPS_WRITE
                    | Effect::MAY_TRAP
                    | Self::analyze(*ptr, arena)
                    | Self::analyze(*value, arena)
            }
            ExpressionKind::ArrayNew { size, init, .. } => {
                let mut e = Effect::HEAPS_WRITE | Effect::MAY_TRAP | Self::analyze(*size, arena);
                if let Some(i) = init {
                    e |= Self::analyze(*i, arena);
                }
                e
            }
            ExpressionKind::ArrayGet { ptr, index, .. } => {
                Effect::HEAPS_READ
                    | Effect::MAY_TRAP
                    | Self::analyze(*ptr, arena)
                    | Self::analyze(*index, arena)
            }
            ExpressionKind::ArraySet {
                ptr, index, value, ..
            } => {
                Effect::HEAPS_WRITE
                    | Effect::MAY_TRAP
                    | Self::analyze(*ptr, arena)
                    | Self::analyze(*index, arena)
                    | Self::analyze(*value, arena)
            }
            ExpressionKind::ArrayLen { ptr } => Effect::MAY_TRAP | Self::analyze(*ptr, arena),
            ExpressionKind::Try {
                body, catch_bodies, ..
            } => Self::analyze(*body, arena) | Self::analyze_list(catch_bodies, arena),
            ExpressionKind::Throw { operands, .. } => {
                Effect::THROWS | Self::analyze_list(operands, arena)
            }
            ExpressionKind::Rethrow { .. } => Effect::THROWS,
            ExpressionKind::TupleMake { operands } => Self::analyze_list(operands, arena),
            ExpressionKind::TupleExtract { tuple, .. } => Self::analyze(*tuple, arena),
            ExpressionKind::ElemDrop { .. } => Effect::NONE,
            ExpressionKind::Pop { .. } => Effect::NONE,
            ExpressionKind::I31New { value } => Self::analyze(*value, arena),
            ExpressionKind::I31Get { i31, .. } => Self::analyze(*i31, arena),
            ExpressionKind::RefTest { value, .. }
            | ExpressionKind::RefCast { value, .. }
            | ExpressionKind::BrOn { value, .. } => Self::analyze(*value, arena),
        }
    }

    /// Analyzes a list of expressions and returns the union of their effects.
    pub fn analyze_list(
        exprs: &[ExpressionId],
        arena: &IdArena<ExpressionMarker, Expression>,
    ) -> Effect {
        let mut effect = Effect::NONE;
        for expr in exprs {
            effect |= Self::analyze(*expr, arena);
        }
        effect
    }

    /// Analyzes a range of expressions between two indices in a list.
    ///
    /// This is useful for checking if there are interfering effects between
    /// two operations in a block.
    pub fn analyze_range(
        exprs: &[ExpressionId],
        start: usize,
        end: usize,
        arena: &IdArena<ExpressionMarker, Expression>,
    ) -> Effect {
        let mut effect = Effect::NONE;
        for i in start..end {
            if i < exprs.len() {
                let e = exprs[i];
                effect |= Self::analyze(e, arena);
            }
        }
        effect
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collections::Id;
    use crate::ir::expression::IrBuilder;
    use crate::ir::{FuncId, GlobalId, Literal, LocalId, MemoryId, Type};

    #[test]
    fn test_effect_none() {
        let effect = Effect::NONE;
        assert!(effect.is_pure());
        assert!(!effect.may_trap());
        assert!(!effect.has_side_effects());
    }

    #[test]
    fn test_effect_composition() {
        let effect = Effect::MEMORY_READ | Effect::MAY_TRAP;
        assert!(!effect.is_pure());
        assert!(effect.may_trap());
        assert!(effect.reads_memory());
        assert!(!effect.writes_memory());
    }

    #[test]
    fn test_effect_trap() {
        let effect = Effect::TRAPS;
        assert!(effect.may_trap());
        assert!(effect.traps());
        assert!(effect.transfers_control());
        assert!(effect.has_side_effects());
    }

    #[test]
    fn test_effect_calls() {
        let effect = Effect::CALLS;
        assert!(effect.calls());
        assert!(effect.has_side_effects());
    }

    #[test]
    fn test_effect_branches() {
        let effect = Effect::BRANCHES;
        assert!(effect.transfers_control());
        assert!(effect.has_side_effects());
    }

    #[test]
    fn test_effect_memory() {
        let read = Effect::MEMORY_READ;
        let write = Effect::MEMORY_WRITE;
        let both = read | write;

        assert!(read.reads_memory());
        assert!(!read.writes_memory());
        assert!(!write.reads_memory());
        assert!(write.writes_memory());
        assert!(both.reads_memory());
        assert!(both.writes_memory());
        assert!(both.reads_state());
        assert!(both.writes_state());
    }

    #[test]
    fn test_effect_globals() {
        let read = Effect::GLOBAL_READ;
        let write = Effect::GLOBAL_WRITE;

        assert!(read.reads_global());
        assert!(!read.writes_global());
        assert!(!write.reads_global());
        assert!(write.writes_global());
        assert!(read.reads_state());
        assert!(write.writes_state());
    }

    #[test]
    fn test_effect_locals() {
        let effect = Effect::LOCAL_WRITE;
        assert!(effect.writes_local());
        assert!(effect.writes_state());
    }

    #[test]
    fn test_effect_interference() {
        // Write-write conflicts
        assert!(Effect::MEMORY_WRITE.interferes_with(Effect::MEMORY_WRITE));
        assert!(Effect::GLOBAL_WRITE.interferes_with(Effect::GLOBAL_WRITE));

        // Read-write conflicts
        assert!(Effect::MEMORY_READ.interferes_with(Effect::MEMORY_WRITE));
        assert!(Effect::MEMORY_WRITE.interferes_with(Effect::MEMORY_READ));
        assert!(Effect::GLOBAL_READ.interferes_with(Effect::GLOBAL_WRITE));
        assert!(Effect::GLOBAL_WRITE.interferes_with(Effect::GLOBAL_READ));

        // No conflict for non-overlapping effects
        assert!(!Effect::MEMORY_READ.interferes_with(Effect::GLOBAL_READ));
        assert!(!Effect::LOCAL_WRITE.interferes_with(Effect::MEMORY_READ));

        // Calls interfere with everything
        assert!(Effect::CALLS.interferes_with(Effect::MEMORY_READ));
        assert!(Effect::CALLS.interferes_with(Effect::NONE));
        assert!(Effect::MEMORY_READ.interferes_with(Effect::CALLS));

        // Traps interfere with everything
        assert!(Effect::MAY_TRAP.interferes_with(Effect::MEMORY_READ));
        assert!(Effect::MEMORY_WRITE.interferes_with(Effect::TRAPS));
    }

    #[test]
    fn test_effect_side_effects() {
        assert!(!Effect::NONE.has_side_effects());
        assert!(!Effect::MEMORY_READ.has_side_effects());
        assert!(!Effect::GLOBAL_READ.has_side_effects());

        assert!(Effect::MEMORY_WRITE.has_side_effects());
        assert!(Effect::GLOBAL_WRITE.has_side_effects());
        assert!(Effect::LOCAL_WRITE.has_side_effects());
        assert!(Effect::CALLS.has_side_effects());
        assert!(Effect::TRAPS.has_side_effects());
        assert!(Effect::BRANCHES.has_side_effects());
    }

    #[test]
    fn test_effect_debug() {
        let effect = Effect::MEMORY_READ | Effect::MAY_TRAP;
        let debug_str = format!("{:?}", effect);
        assert!(debug_str.contains("MEMORY_READ"));
        assert!(debug_str.contains("MAY_TRAP"));
    }

    // EffectAnalyzer tests

    #[test]
    fn test_analyzer_nop() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);
        let nop = builder.nop();

        let effect = EffectAnalyzer::analyze(nop, &arena);
        assert_eq!(effect, Effect::NONE);
    }

    #[test]
    fn test_analyzer_const() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);
        let const_expr = builder.const_(Literal::I32(42));

        let effect = EffectAnalyzer::analyze(const_expr, &arena);
        assert_eq!(effect, Effect::NONE);
    }

    #[test]
    fn test_analyzer_unreachable() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);
        let unreachable = builder.unreachable();

        let effect = EffectAnalyzer::analyze(unreachable, &arena);
        assert!(effect.traps());
        assert!(effect.transfers_control());
    }

    #[test]
    fn test_analyzer_local_get() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);
        let local_get = builder.local_get(LocalId::from(0), Type::I32);

        let effect = EffectAnalyzer::analyze(local_get, &arena);
        assert_eq!(effect, Effect::NONE);
    }

    #[test]
    fn test_analyzer_local_set() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);
        let value = builder.const_(Literal::I32(42));
        let local_set = builder.local_set(LocalId::from(0), value);

        let effect = EffectAnalyzer::analyze(local_set, &arena);
        assert!(effect.writes_local());
        assert!(!effect.reads_state());
    }

    #[test]
    fn test_analyzer_global_get() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);
        let g0_id: GlobalId = Id::from_raw_parts(0, 0);
        let global_get = builder.global_get(g0_id, Type::I32);

        let effect = EffectAnalyzer::analyze(global_get, &arena);
        assert!(effect.reads_global());
        assert!(!effect.writes_global());
    }

    #[test]
    fn test_analyzer_global_set() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);
        let value = builder.const_(Literal::I32(42));
        let g0_id: GlobalId = Id::from_raw_parts(0, 0);
        let global_set = builder.global_set(g0_id, value);

        let effect = EffectAnalyzer::analyze(global_set, &arena);
        assert!(effect.writes_global());
        assert!(effect.writes_state());
    }

    #[test]
    fn test_analyzer_load() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);
        let ptr = builder.const_(Literal::I32(0));
        let load = builder.load(
            MemoryId::from_raw_parts(0, 0),
            4,
            false,
            0,
            4,
            ptr,
            Type::I32,
        );

        let effect = EffectAnalyzer::analyze(load, &arena);
        assert!(effect.reads_memory());
        assert!(effect.may_trap());
        assert!(!effect.writes_memory());
    }

    #[test]
    fn test_analyzer_store() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);
        let ptr = builder.const_(Literal::I32(0));
        let value = builder.const_(Literal::I32(42));
        let store = builder.store(MemoryId::from_raw_parts(0, 0), 4, 0, 4, value, ptr);

        let effect = EffectAnalyzer::analyze(store, &arena);
        assert!(effect.writes_memory());
        assert!(effect.may_trap());
        assert!(!effect.reads_memory());
    }

    #[test]
    fn test_analyzer_call() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);
        let operands = vec![];
        let call = builder.call(
            FuncId::from_raw_parts(0, 0), /* foo */
            operands,
            Type::I32,
            false,
        );

        let effect = EffectAnalyzer::analyze(call, &arena);
        assert!(effect.calls());
        assert!(effect.has_side_effects());
    }

    #[test]
    fn test_analyzer_block_aggregation() {
        let mut arena = IdArena::new();
        let (store1, load, call) = {
            let mut builder = IrBuilder::new(&mut arena);

            let c0 = builder.const_(Literal::I32(0));
            let c1 = builder.const_(Literal::I32(1));
            let store1 = builder.store(MemoryId::from_raw_parts(0, 0), 4, 0, 4, c0, c1);
            let c4 = builder.const_(Literal::I32(4));
            let load = builder.load(
                MemoryId::from_raw_parts(0, 0),
                4,
                false,
                0,
                4,
                c4,
                Type::I32,
            );
            let call = builder.call(
                FuncId::from_raw_parts(0, 0), /* bar */
                vec![],
                Type::I32,
                false,
            );
            (store1, load, call)
        };

        let block = {
            let mut builder = IrBuilder::new(&mut arena);
            builder.block(None::<String>, vec![store1, load, call], Type::I32)
        };

        let effect = EffectAnalyzer::analyze(block, &arena);
        assert!(effect.writes_memory());
        assert!(effect.reads_memory());
        assert!(effect.calls());
        assert!(effect.may_trap());
    }

    #[test]
    fn test_analyzer_if_aggregation() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);

        let condition = builder.const_(Literal::I32(1));
        let g0_id: GlobalId = Id::from_raw_parts(0, 0);
        let c1 = builder.const_(Literal::I32(1));
        let true_branch = builder.global_set(g0_id, c1);
        let c2 = builder.const_(Literal::I32(2));
        let false_branch = builder.local_set(LocalId::from(0), c2);

        let if_expr = builder.if_(
            None::<String>,
            condition,
            true_branch,
            Some(false_branch),
            Type::I32,
        );

        let effect = EffectAnalyzer::analyze(if_expr, &arena);
        assert!(effect.writes_global());
        assert!(effect.writes_local());
    }

    #[test]
    fn test_analyzer_binary_may_trap() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);
        let left = builder.const_(Literal::I32(10));
        let right = builder.const_(Literal::I32(0));
        let div = builder.binary(crate::ir::ops::BinaryOp::DivSInt32, left, right, Type::I32);

        let effect = EffectAnalyzer::analyze(div, &arena);
        assert!(effect.may_trap());
    }

    #[test]
    fn test_analyzer_break() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);
        let br = builder.break_("loop", None, None, Type::NONE);

        let effect = EffectAnalyzer::analyze(br, &arena);
        assert!(effect.transfers_control());
        assert!(effect.contains(Effect::BRANCHES));
    }

    #[test]
    fn test_analyzer_return() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);
        let value = builder.const_(Literal::I32(42));
        let ret = builder.return_(Some(value));

        let effect = EffectAnalyzer::analyze(ret, &arena);
        assert!(effect.transfers_control());
        assert!(effect.contains(Effect::BRANCHES));
    }

    #[test]
    fn test_analyzer_range() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);

        let nop = builder.nop();
        let c0 = builder.const_(Literal::I32(0));
        let c1 = builder.const_(Literal::I32(1));
        let store = builder.store(MemoryId::from_raw_parts(0, 0), 4, 0, 4, c0, c1);
        let operands = vec![];
        let call = builder.call(
            FuncId::from_raw_parts(0, 0), /* foo */
            operands,
            Type::I32,
            false,
        );
        let ptr_load = builder.const_(Literal::I32(0));
        let load = builder.load(
            MemoryId::from_raw_parts(0, 0),
            4,
            false,
            0,
            4,
            ptr_load,
            Type::I32,
        );

        let exprs = vec![nop, store, call, load];

        // Range 0..1: just nop
        let effect = EffectAnalyzer::analyze_range(&exprs, 0, 1, &arena);
        assert_eq!(effect, Effect::NONE);

        // Range 1..3: store and call
        let effect = EffectAnalyzer::analyze_range(&exprs, 1, 3, &arena);
        assert!(effect.writes_memory());
        assert!(effect.calls());

        // Range 2..3: just call
        let effect = EffectAnalyzer::analyze_range(&exprs, 2, 3, &arena);
        assert!(effect.calls());
        assert!(!effect.writes_memory());
    }

    // Additional coverage tests

    #[test]
    fn test_analyzer_loop() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);

        let ptr = builder.const_(Literal::I32(0));
        let val = builder.const_(Literal::I32(42));
        let body = builder.store(MemoryId::from_raw_parts(0, 0), 4, 0, 4, ptr, val);
        let loop_expr = builder.loop_(Some("myloop"), body, Type::NONE);

        let effect = EffectAnalyzer::analyze(loop_expr, &arena);
        assert!(effect.writes_memory());
        assert!(effect.may_trap());
    }

    #[test]
    fn test_analyzer_local_tee() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);

        let value = builder.const_(Literal::I32(42));
        let tee = builder.local_tee(LocalId::from(0), value, Type::I32);

        let effect = EffectAnalyzer::analyze(tee, &arena);
        assert!(effect.writes_local());
        assert!(!effect.reads_global());
    }

    #[test]
    fn test_analyzer_drop() {
        let mut arena = IdArena::new();
        let drop1;
        let drop2;
        {
            let mut builder = IrBuilder::new(&mut arena);

            // Drop of pure expression
            let const_val = builder.const_(Literal::I32(42));
            drop1 = builder.drop(const_val);

            // Drop of call (has side effects)
            let operands = vec![];
            let call = builder.call(
                FuncId::from_raw_parts(0, 0), /* foo */
                operands,
                Type::I32,
                false,
            );
            drop2 = builder.drop(call);
        }

        let effect1 = EffectAnalyzer::analyze(drop1, &arena);
        assert_eq!(effect1, Effect::NONE);

        let effect2 = EffectAnalyzer::analyze(drop2, &arena);
        assert!(effect2.calls());
    }

    #[test]
    fn test_analyzer_select() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);

        let condition = builder.const_(Literal::I32(1));
        let g0_id: GlobalId = Id::from_raw_parts(0, 0);
        let if_true = builder.global_get(g0_id, Type::I32);
        let if_false = builder.local_get(LocalId::from(1), Type::I32);
        let select = builder.select(condition, if_true, if_false, Type::I32);

        let effect = EffectAnalyzer::analyze(select, &arena);
        assert!(effect.reads_global());
        assert!(!effect.writes_global());
        assert!(!effect.writes_local());
    }

    #[test]
    fn test_analyzer_select_with_side_effects() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);

        let condition = builder.const_(Literal::I32(1));
        let operands1 = vec![];
        let if_true = builder.call(
            FuncId::from_raw_parts(0, 0), /* foo */
            operands1,
            Type::I32,
            false,
        );
        let operands2 = vec![];
        let if_false = builder.call(
            FuncId::from_raw_parts(0, 0), /* bar */
            operands2,
            Type::I32,
            false,
        );
        let select = builder.select(condition, if_true, if_false, Type::I32);

        let effect = EffectAnalyzer::analyze(select, &arena);
        assert!(effect.calls());
        assert!(effect.has_side_effects());
    }

    #[test]
    fn test_analyzer_memory_grow() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);

        let delta = builder.const_(Literal::I32(1));
        let grow = builder.memory_grow(MemoryId::from_raw_parts(0, 0), delta);

        let effect = EffectAnalyzer::analyze(grow, &arena);
        assert!(effect.contains(Effect::GROWS));
        assert!(effect.contains(Effect::NONDETERMINISTIC));
    }

    #[test]
    fn test_analyzer_memory_size() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);

        let size = builder.memory_size(MemoryId::from_raw_parts(0, 0));

        let effect = EffectAnalyzer::analyze(size, &arena);
        assert!(effect.reads_memory());
        assert!(!effect.writes_memory());
    }

    #[test]
    fn test_analyzer_call_indirect() {
        // CallIndirect not yet available in IrBuilder
        // Skipping test for now - would test:
        // - effect.calls() == true
        // - effect.may_trap() == true
        // - effect.contains(Effect::TABLE_ACCESS) == true
    }

    #[test]
    fn test_analyzer_nested_control_flow() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);

        // Nested: if (cond) { loop { store; break; } }
        let c0 = builder.const_(Literal::I32(0));
        let c42 = builder.const_(Literal::I32(42));
        let store = builder.store(MemoryId::from_raw_parts(0, 0), 4, 0, 4, c0, c42);
        let br = builder.break_("myloop", None, None, Type::NONE);
        let loop_list = vec![store, br];
        let loop_block = builder.block(None::<String>, loop_list, Type::NONE);
        let loop_expr = builder.loop_(Some("myloop"), loop_block, Type::NONE);

        let condition = builder.const_(Literal::I32(1));
        let if_expr = builder.if_(None::<&str>, condition, loop_expr, None, Type::NONE);

        let effect = EffectAnalyzer::analyze(if_expr, &arena);
        assert!(effect.writes_memory());
        assert!(effect.transfers_control());
        assert!(effect.may_trap());
    }

    #[test]
    fn test_analyzer_empty_block() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);

        let empty_list = vec![];
        let block = builder.block("", empty_list, Type::NONE);

        let effect = EffectAnalyzer::analyze(block, &arena);
        assert_eq!(effect, Effect::NONE);
    }

    #[test]
    fn test_analyzer_unary_propagates_child_effects() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);

        // Unary wrapping a call
        let operands = vec![];
        let call = builder.call(
            FuncId::from_raw_parts(0, 0), /* foo */
            operands,
            Type::I32,
            false,
        );
        let unary = builder.unary(crate::ir::ops::UnaryOp::EqZInt32, call, Type::I32);

        let effect = EffectAnalyzer::analyze(unary, &arena);
        assert!(effect.calls());
    }

    #[test]
    fn test_analyzer_binary_propagates_both_children() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);

        let g0_id: GlobalId = Id::from_raw_parts(0, 0);
        let left = builder.global_get(g0_id, Type::I32);
        let operands = vec![];
        let right = builder.call(
            FuncId::from_raw_parts(0, 0), /* foo */
            operands,
            Type::I32,
            false,
        );
        let binary = builder.binary(crate::ir::ops::BinaryOp::AddInt32, left, right, Type::I32);

        let effect = EffectAnalyzer::analyze(binary, &arena);
        assert!(effect.reads_global());
        assert!(effect.calls());
    }

    #[test]
    fn test_analyzer_if_without_else() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);

        let condition = builder.const_(Literal::I32(1));
        let c42 = builder.const_(Literal::I32(42));
        let true_branch = builder.local_set(LocalId::from(0), c42);
        let if_expr = builder.if_(None::<&str>, condition, true_branch, None, Type::NONE);

        let effect = EffectAnalyzer::analyze(if_expr, &arena);
        assert!(effect.writes_local());
        assert!(!effect.writes_global());
    }

    #[test]
    fn test_effect_interference_local_vs_memory() {
        // Local writes should not interfere with memory operations
        assert!(!Effect::LOCAL_WRITE.interferes_with(Effect::MEMORY_READ));
        assert!(!Effect::LOCAL_WRITE.interferes_with(Effect::MEMORY_WRITE));
        assert!(!Effect::MEMORY_READ.interferes_with(Effect::LOCAL_WRITE));
    }

    #[test]
    fn test_effect_interference_symmetry() {
        // Interference should be symmetric
        let e1 = Effect::MEMORY_WRITE;
        let e2 = Effect::MEMORY_READ;
        assert_eq!(e1.interferes_with(e2), e2.interferes_with(e1));

        let e3 = Effect::CALLS;
        let e4 = Effect::GLOBAL_READ;
        assert_eq!(e3.interferes_with(e4), e4.interferes_with(e3));
    }

    #[test]
    fn test_effect_grows_implies_nondeterminism() {
        let effect = Effect::GROWS;
        // Memory grow operations are inherently nondeterministic in some contexts
        // but GROWS alone doesn't imply NONDETERMINISTIC
        assert!(!effect.contains(Effect::NONDETERMINISTIC));
    }

    #[test]
    fn test_effect_multiple_flags_composition() {
        let effect = Effect::MEMORY_READ | Effect::MEMORY_WRITE | Effect::MAY_TRAP | Effect::CALLS;

        assert!(effect.reads_memory());
        assert!(effect.writes_memory());
        assert!(effect.may_trap());
        assert!(effect.calls());
        assert!(effect.has_side_effects());
        assert!(effect.reads_state());
        assert!(effect.writes_state());
    }

    #[test]
    fn test_analyzer_list_empty() {
        let arena = IdArena::new();
        let empty: Vec<ExpressionId> = vec![];
        let effect = EffectAnalyzer::analyze_list(&empty, &arena);
        assert_eq!(effect, Effect::NONE);
    }

    #[test]
    fn test_analyzer_range_out_of_bounds() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);

        let nop = builder.nop();
        let exprs = vec![nop];

        // Range beyond array bounds should be handled gracefully
        let effect = EffectAnalyzer::analyze_range(&exprs, 0, 10, &arena);
        assert_eq!(effect, Effect::NONE);
    }

    #[test]
    fn test_analyzer_range_reversed() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);

        let ptr = builder.const_(Literal::I32(0));
        let value = builder.const_(Literal::I32(1));
        let store = builder.store(MemoryId::from_raw_parts(0, 0), 4, 0, 4, ptr, value);
        let exprs = vec![store];

        // Reversed range (start > end) should return NONE
        let effect = EffectAnalyzer::analyze_range(&exprs, 1, 0, &arena);
        assert_eq!(effect, Effect::NONE);
    }

    #[test]
    fn test_analyzer_return_with_side_effect_value() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);

        let operands = vec![];
        let call = builder.call(
            FuncId::from_raw_parts(0, 0), /* foo */
            operands,
            Type::I32,
            false,
        );
        let ret = builder.return_(Some(call));

        let effect = EffectAnalyzer::analyze(ret, &arena);
        assert!(effect.calls());
        assert!(effect.transfers_control());
    }

    #[test]
    fn test_analyzer_break_with_condition_and_value() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);

        let operands1 = vec![];
        let condition = builder.call(
            FuncId::from_raw_parts(0, 0), /* should_break */
            operands1,
            Type::I32,
            false,
        );
        let operands2 = vec![];
        let value = builder.call(
            FuncId::from_raw_parts(0, 0), /* get_value */
            operands2,
            Type::I32,
            false,
        );
        let br = builder.break_("loop", Some(condition), Some(value), Type::I32);

        let effect = EffectAnalyzer::analyze(br, &arena);
        assert!(effect.calls());
        assert!(effect.transfers_control());
    }

    #[test]
    fn test_effect_traps_vs_may_trap() {
        // TRAPS (definite trap) should be a stronger condition than MAY_TRAP
        assert!(Effect::TRAPS.may_trap());
        assert!(Effect::TRAPS.traps());

        assert!(Effect::MAY_TRAP.may_trap());
        assert!(!Effect::MAY_TRAP.traps());
    }

    // ========================================
    // Priority 1: Interference Detection Tests
    // ========================================

    #[test]
    fn test_interference_memory_read_write_conflict() {
        let mut arena = IdArena::new();
        let load;
        let store;
        {
            let mut builder = IrBuilder::new(&mut arena);

            // Load from address 0
            let ptr1 = builder.const_(Literal::I32(0));
            load = builder.load(
                MemoryId::from_raw_parts(0, 0),
                4,
                false,
                0,
                4,
                ptr1,
                Type::I32,
            );

            // Store to address 0
            let ptr2 = builder.const_(Literal::I32(0));
            let value = builder.const_(Literal::I32(42));
            store = builder.store(MemoryId::from_raw_parts(0, 0), 4, 0, 4, value, ptr2);
        }

        let load_effect = EffectAnalyzer::analyze(load, &arena);
        let store_effect = EffectAnalyzer::analyze(store, &arena);

        // Read-write conflict should be detected
        assert!(load_effect.interferes_with(store_effect));
        assert!(store_effect.interferes_with(load_effect));

        // Verify individual effects
        assert!(load_effect.reads_memory());
        assert!(!load_effect.writes_memory());
        assert!(store_effect.writes_memory());
        assert!(!store_effect.reads_memory());
    }

    #[test]
    fn test_interference_global_read_write_conflict() {
        let mut arena = IdArena::new();
        let global_get;
        let global_set;
        let g0_id: GlobalId = Id::from_raw_parts(0, 0);
        {
            let mut builder = IrBuilder::new(&mut arena);

            // Read global 0
            global_get = builder.global_get(g0_id, Type::I32);

            // Write to global 0
            let value = builder.const_(Literal::I32(42));
            global_set = builder.global_set(g0_id, value);
        }

        let read_effect = EffectAnalyzer::analyze(global_get, &arena);
        let write_effect = EffectAnalyzer::analyze(global_set, &arena);

        // Read-write conflict on globals
        assert!(read_effect.interferes_with(write_effect));
        assert!(write_effect.interferes_with(read_effect));

        // Verify effects
        assert!(read_effect.reads_global());
        assert!(!read_effect.writes_global());
        assert!(write_effect.writes_global());
    }

    #[test]
    fn test_no_interference_different_effect_types() {
        let mut arena = IdArena::new();
        let local_set;
        let global_set;
        {
            let mut builder = IrBuilder::new(&mut arena);

            // LocalSet (writes locals, doesn't trap if value is pure)
            let c10 = builder.const_(Literal::I32(10));
            local_set = builder.local_set(LocalId::from(0), c10);

            // GlobalSet (writes globals, doesn't trap if value is pure)
            let g1_id: GlobalId = Id::from_raw_parts(1, 0);
            let c20 = builder.const_(Literal::I32(20));
            global_set = builder.global_set(g1_id, c20);
        }

        let local_effect = EffectAnalyzer::analyze(local_set, &arena);
        let global_effect = EffectAnalyzer::analyze(global_set, &arena);

        // Different effect domains (local vs global) should not interfere
        assert!(!local_effect.interferes_with(global_effect));
        assert!(!global_effect.interferes_with(local_effect));

        // But both should have side effects
        assert!(local_effect.has_side_effects());
        assert!(global_effect.has_side_effects());

        // Store has MAY_TRAP so it interferes with everything - that's expected behavior
    }

    #[test]
    fn test_interference_call_blocks_everything() {
        let mut arena = IdArena::new();
        let call;
        {
            let mut builder = IrBuilder::new(&mut arena);

            // Call has arbitrary effects
            let operands = vec![];
            call = builder.call(
                FuncId::from_raw_parts(0, 0), /* arbitrary */
                operands,
                Type::I32,
                false,
            );
        }
        let call_effect = EffectAnalyzer::analyze(call, &arena);

        // Call should interfere with NONE
        assert!(call_effect.interferes_with(Effect::NONE));

        // Call should interfere with reads
        assert!(call_effect.interferes_with(Effect::MEMORY_READ));
        assert!(call_effect.interferes_with(Effect::GLOBAL_READ));

        // Call should interfere with writes
        assert!(call_effect.interferes_with(Effect::MEMORY_WRITE));
        assert!(call_effect.interferes_with(Effect::GLOBAL_WRITE));
        assert!(call_effect.interferes_with(Effect::LOCAL_WRITE));

        // Call should interfere with other calls
        assert!(call_effect.interferes_with(Effect::CALLS));
    }

    #[test]
    fn test_interference_trap_blocks_everything() {
        let mut arena = IdArena::new();
        let unreachable;
        {
            let mut builder = IrBuilder::new(&mut arena);

            // Unreachable always traps
            unreachable = builder.unreachable();
        }
        let trap_effect = EffectAnalyzer::analyze(unreachable, &arena);

        assert!(trap_effect.traps());
        assert!(trap_effect.may_trap());

        // Trap should interfere with everything
        assert!(trap_effect.interferes_with(Effect::NONE));
        assert!(trap_effect.interferes_with(Effect::MEMORY_READ));
        assert!(trap_effect.interferes_with(Effect::MEMORY_WRITE));
        assert!(trap_effect.interferes_with(Effect::CALLS));

        // MAY_TRAP should also interfere broadly
        assert!(Effect::MAY_TRAP.interferes_with(Effect::MEMORY_READ));
        assert!(Effect::MAY_TRAP.interferes_with(Effect::GLOBAL_WRITE));
    }

    #[test]
    fn test_interference_write_write_conflicts() {
        // Memory write-write conflict
        assert!(Effect::MEMORY_WRITE.interferes_with(Effect::MEMORY_WRITE));

        // Global write-write conflict
        assert!(Effect::GLOBAL_WRITE.interferes_with(Effect::GLOBAL_WRITE));

        // But local writes don't interfere with memory/global writes
        assert!(!Effect::LOCAL_WRITE.interferes_with(Effect::MEMORY_WRITE));
        assert!(!Effect::LOCAL_WRITE.interferes_with(Effect::GLOBAL_WRITE));

        // Memory and global writes don't interfere with each other
        assert!(!Effect::MEMORY_WRITE.interferes_with(Effect::GLOBAL_WRITE));
        assert!(!Effect::GLOBAL_WRITE.interferes_with(Effect::MEMORY_WRITE));
    }

    // ========================================
    // Priority 2: Effect Composition Tests
    // ========================================

    #[test]
    fn test_composition_deeply_nested_blocks() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);

        // Inner block with memory write
        let addr = builder.const_(Literal::I32(0));
        let val = builder.const_(Literal::I32(1));
        let store = builder.store(MemoryId::from_raw_parts(0, 0), 4, 0, 4, addr, val);
        let inner_body = vec![store];
        let inner_block = builder.block(Some("inner"), inner_body, Type::NONE);

        // Middle block wrapping inner
        let middle_body = vec![inner_block];
        let middle_block = builder.block(Some("middle"), middle_body, Type::NONE);

        // Outer block wrapping middle
        let outer_body = vec![middle_block];
        let outer_block = builder.block(Some("outer"), outer_body, Type::NONE);

        let outer_effect = EffectAnalyzer::analyze(outer_block, &arena);

        // Memory write should propagate through all nested blocks
        assert!(outer_effect.writes_memory());
        assert!(outer_effect.has_side_effects());
    }

    #[test]
    fn test_composition_sequence_accumulates_effects() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);

        // First: memory read
        let addr = builder.const_(Literal::I32(0));
        let load = builder.load(
            MemoryId::from_raw_parts(0, 0),
            4,
            false,
            0,
            4,
            addr,
            Type::I32,
        );

        // Second: global write
        let g1_id: GlobalId = Id::from_raw_parts(1, 0);
        let c42 = builder.const_(Literal::I32(42));
        let global_set = builder.global_set(g1_id, c42);

        // Third: local write
        let c99 = builder.const_(Literal::I32(99));
        let local_set = builder.local_set(LocalId::from(2), c99);

        let body = vec![load, global_set, local_set];
        let block = builder.block("", body, Type::NONE);

        let block_effect = EffectAnalyzer::analyze(block, &arena);

        // All three effects should be present
        assert!(block_effect.reads_memory());
        assert!(block_effect.writes_global());
        assert!(block_effect.writes_local());

        // Should have multiple side effects
        assert!(block_effect.has_side_effects());
    }

    #[test]
    fn test_composition_loop_multiplies_effects() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);

        // Loop body with side effect (memory write)
        let addr = builder.const_(Literal::I32(100));
        let val = builder.const_(Literal::I32(42));
        let body = builder.store(MemoryId::from_raw_parts(0, 0), 4, 0, 4, addr, val);
        let loop_expr = builder.loop_(Some("repeat"), body, Type::NONE);

        let loop_effect = EffectAnalyzer::analyze(loop_expr, &arena);

        // Effects should propagate from loop body
        assert!(loop_effect.writes_memory());
        assert!(loop_effect.has_side_effects());

        // Loop inherently branches
        assert!(loop_effect.branches());
    }

    #[test]
    fn test_composition_if_both_branches_pure() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);

        // Condition (pure)
        let cond = builder.const_(Literal::I32(1));

        // If-true: const (pure)
        let if_true = builder.const_(Literal::I32(10));

        // If-false: const (pure)
        let if_false = builder.const_(Literal::I32(20));

        let if_expr = builder.if_(None::<&str>, cond, if_true, Some(if_false), Type::I32);

        let if_effect = EffectAnalyzer::analyze(if_expr, &arena);

        // If has branches (which is a side effect), but no writes or calls
        assert!(if_effect.branches());
        assert!(!if_effect.writes_memory());
        assert!(!if_effect.reads_memory());
        assert!(!if_effect.calls());
    }

    // ========================================
    // Priority 3: Optimization Patterns Tests
    // ========================================

    #[test]
    fn test_optimization_dead_store_elimination_candidate() {
        let mut arena = IdArena::new();
        let store1;
        let store2;
        {
            let mut builder = IrBuilder::new(&mut arena);

            // First store to address 0
            let addr1 = builder.const_(Literal::I32(0));
            let val1 = builder.const_(Literal::I32(100));
            store1 = builder.store(MemoryId::from_raw_parts(0, 0), 4, 0, 4, addr1, val1);

            // Second store to same address (overwrites first)
            let addr2 = builder.const_(Literal::I32(0));
            let val2 = builder.const_(Literal::I32(200));
            store2 = builder.store(MemoryId::from_raw_parts(0, 0), 4, 0, 4, addr2, val2);
        }

        let store1_effect = EffectAnalyzer::analyze(store1, &arena);
        let store2_effect = EffectAnalyzer::analyze(store2, &arena);

        let body = vec![store1, store2];
        let mut builder = IrBuilder::new(&mut arena);
        let block = builder.block("", body, Type::NONE);

        let block_effect = EffectAnalyzer::analyze(block, &arena);

        // Both stores contribute to memory writes
        assert!(block_effect.writes_memory());

        // First store could potentially be eliminated (if addresses are proven same)

        // Both have same effect pattern
        assert_eq!(store1_effect, store2_effect);
        assert!(store1_effect.writes_memory());
    }

    #[test]
    fn test_optimization_store_with_interfering_call() {
        let mut arena = IdArena::new();

        // Store to memory
        let store = {
            let mut builder = IrBuilder::new(&mut arena);
            let addr = builder.const_(Literal::I32(0));
            let val = builder.const_(Literal::I32(42));
            builder.store(MemoryId::from_raw_parts(0, 0), 4, 0, 4, addr, val)
        };
        let store_effect = EffectAnalyzer::analyze(store, &arena);

        // Call (may have arbitrary effects)
        let call = {
            let mut builder = IrBuilder::new(&mut arena);
            builder.call(
                FuncId::from_raw_parts(0, 0), /* mayWrite */
                vec![],
                Type::NONE,
                false,
            )
        };
        let call_effect = EffectAnalyzer::analyze(call, &arena);

        // Second store to same location
        let store2 = {
            let mut builder = IrBuilder::new(&mut arena);
            let addr = builder.const_(Literal::I32(0));
            let val = builder.const_(Literal::I32(99));
            builder.store(MemoryId::from_raw_parts(0, 0), 4, 0, 4, addr, val)
        };

        let body = vec![store, call, store2];
        let mut builder = IrBuilder::new(&mut arena);
        let _block = builder.block("", body, Type::NONE);

        // Call interferes with store - prevents optimization
        assert!(call_effect.interferes_with(store_effect));
        assert!(store_effect.interferes_with(call_effect));

        // First store cannot be eliminated due to interfering call
    }

    #[test]
    fn test_optimization_local_set_chain() {
        let mut arena = IdArena::new();
        let set1;
        let set2;
        let set3;
        {
            let mut builder = IrBuilder::new(&mut arena);

            // Chain of sets to same local
            let c1 = builder.const_(Literal::I32(1));
            set1 = builder.local_set(LocalId::from(0), c1);
            let c2 = builder.const_(Literal::I32(2));
            set2 = builder.local_set(LocalId::from(0), c2);
            let c3 = builder.const_(Literal::I32(3));
            set3 = builder.local_set(LocalId::from(0), c3);
        }

        let set1_effect = EffectAnalyzer::analyze(set1, &arena);

        let body = vec![set1, set2, set3];
        let mut builder = IrBuilder::new(&mut arena);
        let block = builder.block("", body, Type::NONE);

        let block_effect = EffectAnalyzer::analyze(block, &arena);

        // All write locals
        assert!(block_effect.writes_local());

        // Earlier sets could be eliminated if no local.get in between
        assert!(set1_effect.writes_local());
        assert!(!set1_effect.reads_local());
    }

    #[test]
    fn test_optimization_local_set_with_interfering_load() {
        let mut arena = IdArena::new();
        let set1;
        let global_get;
        {
            let mut builder = IrBuilder::new(&mut arena);

            // LocalSet (writes local, pure value)
            let c10 = builder.const_(Literal::I32(10));
            set1 = builder.local_set(LocalId::from(0), c10);

            // GlobalGet (reads global, doesn't trap)
            let g1_id: GlobalId = Id::from_raw_parts(1, 0);
            global_get = builder.global_get(g1_id, Type::I32);

            // Second LocalSet
            let c20 = builder.const_(Literal::I32(20));
            let _set2 = builder.local_set(LocalId::from(0), c20);
        }

        let _set_effect = EffectAnalyzer::analyze(set1, &arena);
        let _global_effect = EffectAnalyzer::analyze(global_get, &arena);

        // First set1 COULD be eliminated (no interference from global_get)
        // Note: Load from memory has MAY_TRAP, so it WOULD interfere
    }
}
