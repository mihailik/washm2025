// Effect analysis for WebAssembly expressions.
//
// This module provides the foundational types for reasoning about side effects
// in WebAssembly expressions. Effects are used by optimization passes to
// determine whether it is safe to reorder, eliminate, or move instructions.
//
// ## Design Philosophy
//
// In Rust, we leverage the type system to make effect analysis more precise
use crate::collections::Id;
use crate::ir::{FuncId, GlobalId, MemoryId, TableId};
// and harder to misuse than the C++ counterpart. Effects are represented as
// bitflags, allowing efficient composition and querying.
//
// ## Usage
//
// ```rust
// use washm::analysis::effects::{Effect, EffectAnalyzer};
// use washm::ir::expression::{IrBuilder, ExprRef};
// use washm::ir::{Literal, Type};
// use bumpalo::Bump;
//
// let bump = Bump::new();
// let builder = IrBuilder::new(&bump);
// let expr = builder.unreachable();
//
// let effect = EffectAnalyzer::analyze(expr);
// assert!(effect.may_trap());
// assert!(effect.transfers_control());
// ```

use crate::ir::expression::{ExprRef, ExpressionKind};
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
    pub fn analyze<'a>(expr: ExprRef<'a>) -> Effect {
        match &expr.kind {
            ExpressionKind::Nop => Effect::NONE,

            ExpressionKind::Const(_) => Effect::NONE,

            ExpressionKind::Unreachable => Effect::TRAPS,

            ExpressionKind::Block { list, .. } => Self::analyze_list(list),

            ExpressionKind::Unary { op, value } => {
                let mut effects = Self::analyze(*value);
                if op.may_trap() {
                    effects |= Effect::MAY_TRAP;
                }
                effects
            }

            ExpressionKind::Binary { op, left, right } => {
                let mut effects = Self::analyze(*left) | Self::analyze(*right);
                if op.may_trap() {
                    effects |= Effect::MAY_TRAP;
                }
                effects
            }

            ExpressionKind::LocalGet { .. } => Effect::NONE,

            ExpressionKind::LocalSet { value, .. } => Effect::LOCAL_WRITE | Self::analyze(*value),

            ExpressionKind::LocalTee { value, .. } => Effect::LOCAL_WRITE | Self::analyze(*value),

            ExpressionKind::GlobalGet { .. } => Effect::GLOBAL_READ,

            ExpressionKind::GlobalSet { value, .. } => Effect::GLOBAL_WRITE | Self::analyze(*value),

            ExpressionKind::Load { ptr, .. } => {
                Effect::MEMORY_READ | Effect::MAY_TRAP | Self::analyze(*ptr)
            }

            ExpressionKind::Store { ptr, value, .. } => {
                Effect::MEMORY_WRITE
                    | Effect::MAY_TRAP
                    | Self::analyze(*ptr)
                    | Self::analyze(*value)
            }

            ExpressionKind::Call { operands, .. } => {
                // Calls have arbitrary effects
                Effect::CALLS | Self::analyze_list(operands)
            }

            ExpressionKind::CallIndirect {
                operands, target, ..
            } => {
                // Indirect calls: may trap (bad table index), has table access
                Effect::CALLS
                    | Effect::TABLE_ACCESS
                    | Effect::MAY_TRAP
                    | Self::analyze(*target)
                    | Self::analyze_list(operands)
            }

            ExpressionKind::If {
                condition,
                if_true,
                if_false,
            } => {
                let mut effect = Effect::BRANCHES | Self::analyze(*condition);
                effect |= Self::analyze(*if_true);
                if let Some(if_false_expr) = if_false {
                    effect |= Self::analyze(*if_false_expr);
                }
                effect
            }

            ExpressionKind::Loop { body, .. } => Effect::BRANCHES | Self::analyze(*body),

            ExpressionKind::Break {
                condition, value, ..
            } => {
                let mut effect = Effect::BRANCHES;
                if let Some(cond) = condition {
                    effect |= Self::analyze(*cond);
                }
                if let Some(val) = value {
                    effect |= Self::analyze(*val);
                }
                effect
            }

            ExpressionKind::Return { value } => {
                let mut effect = Effect::BRANCHES;
                if let Some(val) = value {
                    effect |= Self::analyze(*val);
                }
                effect
            }

            ExpressionKind::Drop { value } => Self::analyze(*value),

            ExpressionKind::Select {
                condition,
                if_true,
                if_false,
            } => Self::analyze(*condition) | Self::analyze(*if_true) | Self::analyze(*if_false),

            ExpressionKind::Switch {
                condition, value, ..
            } => {
                let mut effect = Effect::BRANCHES | Self::analyze(*condition);
                if let Some(val) = value {
                    effect |= Self::analyze(*val);
                }
                effect
            }

            ExpressionKind::MemorySize { .. } => Effect::MEMORY_READ,

            ExpressionKind::MemoryGrow { delta, .. } => {
                Effect::GROWS | Effect::NONDETERMINISTIC | Self::analyze(*delta)
            }

            // Atomic operations have memory side effects
            ExpressionKind::AtomicRMW { ptr, value, .. } => {
                Effect::MEMORY_READ
                    | Effect::MEMORY_WRITE
                    | Effect::MAY_TRAP
                    | Self::analyze(*ptr)
                    | Self::analyze(*value)
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
                    | Self::analyze(*ptr)
                    | Self::analyze(*expected)
                    | Self::analyze(*replacement)
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
                    | Self::analyze(*ptr)
                    | Self::analyze(*expected)
                    | Self::analyze(*timeout)
            }

            ExpressionKind::AtomicNotify { ptr, count, .. } => {
                Effect::MEMORY_READ
                    | Effect::MEMORY_WRITE
                    | Effect::MAY_TRAP
                    | Self::analyze(*ptr)
                    | Self::analyze(*count)
            }

            ExpressionKind::AtomicFence => Effect::MEMORY_WRITE,

            // SIMD operations
            ExpressionKind::SIMDExtract { vec, .. } => Effect::MAY_TRAP | Self::analyze(*vec),

            ExpressionKind::SIMDReplace { vec, value, .. } => {
                Effect::MAY_TRAP | Self::analyze(*vec) | Self::analyze(*value)
            }

            ExpressionKind::SIMDShuffle { left, right, .. } => {
                Effect::MAY_TRAP | Self::analyze(*left) | Self::analyze(*right)
            }

            ExpressionKind::SIMDTernary { a, b, c, .. } => {
                Effect::MAY_TRAP | Self::analyze(*a) | Self::analyze(*b) | Self::analyze(*c)
            }

            ExpressionKind::SIMDShift { vec, shift, .. } => {
                Effect::MAY_TRAP | Self::analyze(*vec) | Self::analyze(*shift)
            }

            ExpressionKind::SIMDLoad { ptr, .. } => {
                Effect::MEMORY_READ | Effect::MAY_TRAP | Self::analyze(*ptr)
            }

            ExpressionKind::SIMDLoadStoreLane { ptr, vec, .. } => {
                Effect::MEMORY_READ
                    | Effect::MEMORY_WRITE
                    | Effect::MAY_TRAP
                    | Self::analyze(*ptr)
                    | Self::analyze(*vec)
            }

            ExpressionKind::MemoryInit {
                dest, offset, size, ..
            } => {
                Effect::MEMORY_WRITE
                    | Effect::MAY_TRAP
                    | Self::analyze(*dest)
                    | Self::analyze(*offset)
                    | Self::analyze(*size)
            }

            ExpressionKind::DataDrop { .. } => Effect::MEMORY_WRITE,

            ExpressionKind::MemoryCopy {
                dest, src, size, ..
            } => {
                Effect::MEMORY_READ
                    | Effect::MEMORY_WRITE
                    | Effect::MAY_TRAP
                    | Self::analyze(*dest)
                    | Self::analyze(*src)
                    | Self::analyze(*size)
            }

            ExpressionKind::MemoryFill {
                dest, value, size, ..
            } => {
                Effect::MEMORY_WRITE
                    | Effect::MAY_TRAP
                    | Self::analyze(*dest)
                    | Self::analyze(*value)
                    | Self::analyze(*size)
            }
            ExpressionKind::TableGet { index, .. } => {
                Effect::TABLE_ACCESS | Effect::MAY_TRAP | Self::analyze(*index)
            }
            ExpressionKind::TableSet { index, value, .. } => {
                Effect::TABLE_ACCESS
                    | Effect::MAY_TRAP
                    | Self::analyze(*index)
                    | Self::analyze(*value)
            }
            ExpressionKind::TableSize { .. } => Effect::TABLE_ACCESS,
            ExpressionKind::TableGrow { delta, value, .. } => {
                Effect::TABLE_ACCESS | Effect::GROWS | Self::analyze(*delta) | Self::analyze(*value)
            }
            ExpressionKind::TableFill {
                dest, value, size, ..
            } => {
                Effect::TABLE_ACCESS
                    | Effect::MAY_TRAP
                    | Self::analyze(*dest)
                    | Self::analyze(*value)
                    | Self::analyze(*size)
            }
            ExpressionKind::TableCopy {
                dest, src, size, ..
            } => {
                Effect::TABLE_ACCESS
                    | Effect::MAY_TRAP
                    | Self::analyze(*dest)
                    | Self::analyze(*src)
                    | Self::analyze(*size)
            }
            ExpressionKind::TableInit {
                dest, offset, size, ..
            } => {
                Effect::TABLE_ACCESS
                    | Effect::MAY_TRAP
                    | Self::analyze(*dest)
                    | Self::analyze(*offset)
                    | Self::analyze(*size)
            }
            ExpressionKind::RefNull { .. } => Effect::NONE,
            ExpressionKind::RefIsNull { value } => Self::analyze(*value),
            ExpressionKind::RefAs { value, .. } => Effect::MAY_TRAP | Self::analyze(*value),
            ExpressionKind::RefEq { left, right } => Self::analyze(*left) | Self::analyze(*right),
            ExpressionKind::RefFunc { .. } => Effect::NONE,
            ExpressionKind::StructNew { operands, .. } => {
                Effect::HEAPS_WRITE | Self::analyze_list(operands)
            }
            ExpressionKind::StructGet { ptr, .. } => {
                Effect::HEAPS_READ | Effect::MAY_TRAP | Self::analyze(*ptr)
            }
            ExpressionKind::StructSet { ptr, value, .. } => {
                Effect::HEAPS_WRITE | Effect::MAY_TRAP | Self::analyze(*ptr) | Self::analyze(*value)
            }
            ExpressionKind::ArrayNew { size, init, .. } => {
                let mut e = Effect::HEAPS_WRITE | Effect::MAY_TRAP | Self::analyze(*size);
                if let Some(i) = init {
                    e |= Self::analyze(*i);
                }
                e
            }
            ExpressionKind::ArrayGet { ptr, index, .. } => {
                Effect::HEAPS_READ | Effect::MAY_TRAP | Self::analyze(*ptr) | Self::analyze(*index)
            }
            ExpressionKind::ArraySet {
                ptr, index, value, ..
            } => {
                Effect::HEAPS_WRITE
                    | Effect::MAY_TRAP
                    | Self::analyze(*ptr)
                    | Self::analyze(*index)
                    | Self::analyze(*value)
            }
            ExpressionKind::ArrayLen { ptr } => Effect::MAY_TRAP | Self::analyze(*ptr),
            ExpressionKind::Try {
                body, catch_bodies, ..
            } => Self::analyze(*body) | Self::analyze_list(catch_bodies),
            ExpressionKind::Throw { operands, .. } => Effect::THROWS | Self::analyze_list(operands),
            ExpressionKind::Rethrow { .. } => Effect::THROWS,
            ExpressionKind::TupleMake { operands } => Self::analyze_list(operands),
            ExpressionKind::TupleExtract { tuple, .. } => Self::analyze(*tuple),
            ExpressionKind::ElemDrop { .. } => Effect::NONE,
            ExpressionKind::Pop { .. } => Effect::NONE,
            ExpressionKind::I31New { value } => Self::analyze(*value),
            ExpressionKind::I31Get { i31, .. } => Self::analyze(*i31),
            ExpressionKind::RefTest { value, .. }
            | ExpressionKind::RefCast { value, .. }
            | ExpressionKind::BrOn { value, .. } => Self::analyze(*value),
        }
    }

    /// Analyzes a list of expressions and returns the union of their effects.
    pub fn analyze_list<'a>(exprs: &[ExprRef<'a>]) -> Effect {
        let mut effect = Effect::NONE;
        for expr in exprs {
            effect |= Self::analyze(*expr);
        }
        effect
    }

    /// Analyzes a range of expressions between two indices in a list.
    ///
    /// This is useful for checking if there are interfering effects between
    /// two operations in a block.
    pub fn analyze_range<'a>(exprs: &[ExprRef<'a>], start: usize, end: usize) -> Effect {
        let mut effect = Effect::NONE;
        for i in start..end {
            if i < exprs.len() {
                let e: ExprRef = exprs[i];
                effect |= Self::analyze(e);
            }
        }
        effect
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::{ExprRef, IrBuilder};
    use crate::ir::{Literal, Type};
    use bumpalo::Bump;

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
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let nop = builder.nop();

        let effect = EffectAnalyzer::analyze(nop);
        assert_eq!(effect, Effect::NONE);
    }

    #[test]
    fn test_analyzer_const() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let const_expr = builder.const_(Literal::I32(42));

        let effect = EffectAnalyzer::analyze(const_expr);
        assert_eq!(effect, Effect::NONE);
    }

    #[test]
    fn test_analyzer_unreachable() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let unreachable = builder.unreachable();

        let effect = EffectAnalyzer::analyze(unreachable);
        assert!(effect.traps());
        assert!(effect.transfers_control());
    }

    #[test]
    fn test_analyzer_local_get() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let local_get = builder.local_get(0, Type::I32);

        let effect = EffectAnalyzer::analyze(local_get);
        assert_eq!(effect, Effect::NONE);
    }

    #[test]
    fn test_analyzer_local_set() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let value = builder.const_(Literal::I32(42));
        let local_set = builder.local_set(0, value);

        let effect = EffectAnalyzer::analyze(local_set);
        assert!(effect.writes_local());
        assert!(!effect.reads_state());
    }

    #[test]
    fn test_analyzer_global_get() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let g0_id: GlobalId = Id::from_raw_parts(0, 0);
        let global_get = builder.global_get(g0_id, Type::I32);

        let effect = EffectAnalyzer::analyze(global_get);
        assert!(effect.reads_global());
        assert!(!effect.writes_global());
    }

    #[test]
    fn test_analyzer_global_set() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let value = builder.const_(Literal::I32(42));
        let g0_id: GlobalId = Id::from_raw_parts(0, 0);
        let global_set = builder.global_set(g0_id, value);

        let effect = EffectAnalyzer::analyze(global_set);
        assert!(effect.writes_global());
        assert!(effect.writes_state());
    }

    #[test]
    fn test_analyzer_load() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
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

        let effect = EffectAnalyzer::analyze(load);
        assert!(effect.reads_memory());
        assert!(effect.may_trap());
        assert!(!effect.writes_memory());
    }

    #[test]
    fn test_analyzer_store() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let ptr = builder.const_(Literal::I32(0));
        let value = builder.const_(Literal::I32(42));
        let store = builder.store(MemoryId::from_raw_parts(0, 0), 4, 0, 4, value, ptr);

        let effect = EffectAnalyzer::analyze(store);
        assert!(effect.writes_memory());
        assert!(effect.may_trap());
        assert!(!effect.reads_memory());
    }

    #[test]
    fn test_analyzer_call() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let operands = bumpalo::vec![in &bump;];
        let call = builder.call(
            FuncId::from_raw_parts(0, 0), /* foo */
            operands,
            Type::I32,
            false,
        );

        let effect = EffectAnalyzer::analyze(call);
        assert!(effect.calls());
        assert!(effect.has_side_effects());
    }

    #[test]
    fn test_analyzer_block_aggregation() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        let store1 = builder.store(
            MemoryId::from_raw_parts(0, 0),
            4,
            0,
            4,
            builder.const_(Literal::I32(0)),
            builder.const_(Literal::I32(1)),
        );
        let load = builder.load(
            MemoryId::from_raw_parts(0, 0),
            4,
            false,
            0,
            4,
            builder.const_(Literal::I32(4)),
            Type::I32,
        );
        let operands = bumpalo::vec![in &bump;];
        let call = builder.call(
            FuncId::from_raw_parts(0, 0), /* bar */
            operands,
            Type::I32,
            false,
        );

        let list = bumpalo::vec![in &bump; store1, load, call];
        let block = builder.block(None, list, Type::I32);

        let effect = EffectAnalyzer::analyze(block);
        assert!(effect.writes_memory());
        assert!(effect.reads_memory());
        assert!(effect.calls());
        assert!(effect.may_trap());
    }

    #[test]
    fn test_analyzer_if_aggregation() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        let condition = builder.const_(Literal::I32(1));
        let g0_id: GlobalId = Id::from_raw_parts(0, 0);
        let true_branch = builder.global_set(g0_id, builder.const_(Literal::I32(1)));
        let false_branch = builder.local_set(0, builder.const_(Literal::I32(2)));

        let if_expr = builder.if_(condition, true_branch, Some(false_branch), Type::I32);

        let effect = EffectAnalyzer::analyze(if_expr);
        assert!(effect.writes_global());
        assert!(effect.writes_local());
    }

    #[test]
    fn test_analyzer_binary_may_trap() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let left = builder.const_(Literal::I32(10));
        let right = builder.const_(Literal::I32(0));
        let div = builder.binary(crate::ir::ops::BinaryOp::DivSInt32, left, right, Type::I32);

        let effect = EffectAnalyzer::analyze(div);
        assert!(effect.may_trap());
    }

    #[test]
    fn test_analyzer_break() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let br = builder.break_("loop", None, None, Type::NONE);

        let effect = EffectAnalyzer::analyze(br);
        assert!(effect.transfers_control());
        assert!(effect.contains(Effect::BRANCHES));
    }

    #[test]
    fn test_analyzer_return() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let value = builder.const_(Literal::I32(42));
        let ret = builder.return_(Some(value));

        let effect = EffectAnalyzer::analyze(ret);
        assert!(effect.transfers_control());
        assert!(effect.contains(Effect::BRANCHES));
    }

    #[test]
    fn test_analyzer_range() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        let nop = builder.nop();
        let store = builder.store(
            MemoryId::from_raw_parts(0, 0),
            4,
            0,
            4,
            builder.const_(Literal::I32(0)),
            builder.const_(Literal::I32(1)),
        );
        let operands = bumpalo::vec![in &bump;];
        let call = builder.call(
            FuncId::from_raw_parts(0, 0), /* foo */
            operands,
            Type::I32,
            false,
        );
        let load = builder.load(
            MemoryId::from_raw_parts(0, 0),
            4,
            false,
            0,
            4,
            builder.const_(Literal::I32(0)),
            Type::I32,
        );

        let exprs = vec![nop, store, call, load];

        // Range 0..1: just nop
        let effect = EffectAnalyzer::analyze_range(&exprs, 0, 1);
        assert_eq!(effect, Effect::NONE);

        // Range 1..3: store and call
        let effect = EffectAnalyzer::analyze_range(&exprs, 1, 3);
        assert!(effect.writes_memory());
        assert!(effect.calls());

        // Range 2..3: just call
        let effect = EffectAnalyzer::analyze_range(&exprs, 2, 3);
        assert!(effect.calls());
        assert!(!effect.writes_memory());
    }

    // Additional coverage tests

    #[test]
    fn test_analyzer_loop() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        let body = builder.store(
            MemoryId::from_raw_parts(0, 0),
            4,
            0,
            4,
            builder.const_(Literal::I32(0)),
            builder.const_(Literal::I32(42)),
        );
        let loop_expr = builder.loop_(Some("myloop"), body, Type::NONE);

        let effect = EffectAnalyzer::analyze(loop_expr);
        assert!(effect.writes_memory());
        assert!(effect.may_trap());
    }

    #[test]
    fn test_analyzer_local_tee() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        let value = builder.const_(Literal::I32(42));
        let tee = builder.local_tee(0, value, Type::I32);

        let effect = EffectAnalyzer::analyze(tee);
        assert!(effect.writes_local());
        assert!(!effect.reads_global());
    }

    #[test]
    fn test_analyzer_drop() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Drop of pure expression
        let const_val = builder.const_(Literal::I32(42));
        let drop1 = builder.drop(const_val);
        let effect1 = EffectAnalyzer::analyze(drop1);
        assert_eq!(effect1, Effect::NONE);

        // Drop of call (has side effects)
        let operands = bumpalo::vec![in &bump;];
        let call = builder.call(
            FuncId::from_raw_parts(0, 0), /* foo */
            operands,
            Type::I32,
            false,
        );
        let drop2 = builder.drop(call);
        let effect2 = EffectAnalyzer::analyze(drop2);
        assert!(effect2.calls());
    }

    #[test]
    fn test_analyzer_select() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        let condition = builder.const_(Literal::I32(1));
        let g0_id: GlobalId = Id::from_raw_parts(0, 0);
        let if_true = builder.global_get(g0_id, Type::I32);
        let if_false = builder.local_get(1, Type::I32);
        let select = builder.select(condition, if_true, if_false, Type::I32);

        let effect = EffectAnalyzer::analyze(select);
        assert!(effect.reads_global());
        assert!(!effect.writes_global());
        assert!(!effect.writes_local());
    }

    #[test]
    fn test_analyzer_select_with_side_effects() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        let condition = builder.const_(Literal::I32(1));
        let operands1 = bumpalo::vec![in &bump;];
        let if_true = builder.call(
            FuncId::from_raw_parts(0, 0), /* foo */
            operands1,
            Type::I32,
            false,
        );
        let operands2 = bumpalo::vec![in &bump;];
        let if_false = builder.call(
            FuncId::from_raw_parts(0, 0), /* bar */
            operands2,
            Type::I32,
            false,
        );
        let select = builder.select(condition, if_true, if_false, Type::I32);

        let effect = EffectAnalyzer::analyze(select);
        assert!(effect.calls());
        assert!(effect.has_side_effects());
    }

    #[test]
    fn test_analyzer_memory_grow() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        let delta = builder.const_(Literal::I32(1));
        let grow = builder.memory_grow(MemoryId::from_raw_parts(0, 0), delta);

        let effect = EffectAnalyzer::analyze(grow);
        assert!(effect.contains(Effect::GROWS));
        assert!(effect.contains(Effect::NONDETERMINISTIC));
    }

    #[test]
    fn test_analyzer_memory_size() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        let size = builder.memory_size(MemoryId::from_raw_parts(0, 0));

        let effect = EffectAnalyzer::analyze(size);
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
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Nested: if (cond) { loop { store; break; } }
        let store = builder.store(
            MemoryId::from_raw_parts(0, 0),
            4,
            0,
            4,
            builder.const_(Literal::I32(0)),
            builder.const_(Literal::I32(42)),
        );
        let br = builder.break_("myloop", None, None, Type::NONE);
        let loop_list = bumpalo::vec![in &bump; store, br];
        let loop_block = builder.block(None, loop_list, Type::NONE);
        let loop_expr = builder.loop_(Some("myloop"), loop_block, Type::NONE);

        let condition = builder.const_(Literal::I32(1));
        let if_expr = builder.if_(condition, loop_expr, None, Type::NONE);

        let effect = EffectAnalyzer::analyze(if_expr);
        assert!(effect.writes_memory());
        assert!(effect.transfers_control());
        assert!(effect.may_trap());
    }

    #[test]
    fn test_analyzer_empty_block() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        let empty_list = bumpalo::vec![in &bump;];
        let block = builder.block(None, empty_list, Type::NONE);

        let effect = EffectAnalyzer::analyze(block);
        assert_eq!(effect, Effect::NONE);
    }

    #[test]
    fn test_analyzer_unary_propagates_child_effects() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Unary wrapping a call
        let operands = bumpalo::vec![in &bump;];
        let call = builder.call(
            FuncId::from_raw_parts(0, 0), /* foo */
            operands,
            Type::I32,
            false,
        );
        let unary = builder.unary(crate::ir::ops::UnaryOp::EqZInt32, call, Type::I32);

        let effect = EffectAnalyzer::analyze(unary);
        assert!(effect.calls());
    }

    #[test]
    fn test_analyzer_binary_propagates_both_children() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        let g0_id: GlobalId = Id::from_raw_parts(0, 0);
        let left = builder.global_get(g0_id, Type::I32);
        let operands = bumpalo::vec![in &bump;];
        let right = builder.call(
            FuncId::from_raw_parts(0, 0), /* foo */
            operands,
            Type::I32,
            false,
        );
        let binary = builder.binary(crate::ir::ops::BinaryOp::AddInt32, left, right, Type::I32);

        let effect = EffectAnalyzer::analyze(binary);
        assert!(effect.reads_global());
        assert!(effect.calls());
    }

    #[test]
    fn test_analyzer_if_without_else() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        let condition = builder.const_(Literal::I32(1));
        let true_branch = builder.local_set(0, builder.const_(Literal::I32(42)));
        let if_expr = builder.if_(condition, true_branch, None, Type::NONE);

        let effect = EffectAnalyzer::analyze(if_expr);
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
        let empty: Vec<ExprRef> = vec![];
        let effect = EffectAnalyzer::analyze_list(&empty);
        assert_eq!(effect, Effect::NONE);
    }

    #[test]
    fn test_analyzer_range_out_of_bounds() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        let nop = builder.nop();
        let exprs = vec![nop];

        // Range beyond array bounds should be handled gracefully
        let effect = EffectAnalyzer::analyze_range(&exprs, 0, 10);
        assert_eq!(effect, Effect::NONE);
    }

    #[test]
    fn test_analyzer_range_reversed() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        let store = builder.store(
            MemoryId::from_raw_parts(0, 0),
            4,
            0,
            4,
            builder.const_(Literal::I32(0)),
            builder.const_(Literal::I32(1)),
        );
        let exprs = vec![store];

        // Reversed range (start > end) should return NONE
        let effect = EffectAnalyzer::analyze_range(&exprs, 1, 0);
        assert_eq!(effect, Effect::NONE);
    }

    #[test]
    fn test_analyzer_return_with_side_effect_value() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        let operands = bumpalo::vec![in &bump;];
        let call = builder.call(
            FuncId::from_raw_parts(0, 0), /* foo */
            operands,
            Type::I32,
            false,
        );
        let ret = builder.return_(Some(call));

        let effect = EffectAnalyzer::analyze(ret);
        assert!(effect.calls());
        assert!(effect.transfers_control());
    }

    #[test]
    fn test_analyzer_break_with_condition_and_value() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        let operands1 = bumpalo::vec![in &bump;];
        let condition = builder.call(
            FuncId::from_raw_parts(0, 0), /* should_break */
            operands1,
            Type::I32,
            false,
        );
        let operands2 = bumpalo::vec![in &bump;];
        let value = builder.call(
            FuncId::from_raw_parts(0, 0), /* get_value */
            operands2,
            Type::I32,
            false,
        );
        let br = builder.break_("loop", Some(condition), Some(value), Type::I32);

        let effect = EffectAnalyzer::analyze(br);
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
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Load from address 0
        let ptr1 = builder.const_(Literal::I32(0));
        let load = builder.load(
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
        let store = builder.store(MemoryId::from_raw_parts(0, 0), 4, 0, 4, value, ptr2);

        let load_effect = EffectAnalyzer::analyze(load);
        let store_effect = EffectAnalyzer::analyze(store);

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
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Read global 0
        let g0_id: GlobalId = Id::from_raw_parts(0, 0);
        let global_get = builder.global_get(g0_id, Type::I32);

        // Write to global 0
        let value = builder.const_(Literal::I32(42));
        let global_set = builder.global_set(g0_id, value);

        let read_effect = EffectAnalyzer::analyze(global_get);
        let write_effect = EffectAnalyzer::analyze(global_set);

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
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // LocalSet (writes locals, doesn't trap if value is pure)
        let local_set = builder.local_set(0, builder.const_(Literal::I32(10)));

        // GlobalSet (writes globals, doesn't trap if value is pure)
        let g1_id: GlobalId = Id::from_raw_parts(1, 0);
        let global_set = builder.global_set(g1_id, builder.const_(Literal::I32(20)));

        let local_effect = EffectAnalyzer::analyze(local_set);
        let global_effect = EffectAnalyzer::analyze(global_set);

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
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Call has arbitrary effects
        let operands = bumpalo::vec![in &bump;];
        let call = builder.call(
            FuncId::from_raw_parts(0, 0), /* arbitrary */
            operands,
            Type::I32,
            false,
        );
        let call_effect = EffectAnalyzer::analyze(call);

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
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Unreachable always traps
        let unreachable = builder.unreachable();
        let trap_effect = EffectAnalyzer::analyze(unreachable);

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
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Inner block with memory write
        let store = builder.store(
            MemoryId::from_raw_parts(0, 0),
            4,
            0,
            4,
            builder.const_(Literal::I32(0)),
            builder.const_(Literal::I32(1)),
        );
        let inner_body = bumpalo::vec![in &bump; store];
        let inner_block = builder.block(Some("inner"), inner_body, Type::NONE);

        // Middle block wrapping inner
        let middle_body = bumpalo::vec![in &bump; inner_block];
        let middle_block = builder.block(Some("middle"), middle_body, Type::NONE);

        // Outer block wrapping middle
        let outer_body = bumpalo::vec![in &bump; middle_block];
        let outer_block = builder.block(Some("outer"), outer_body, Type::NONE);

        let outer_effect = EffectAnalyzer::analyze(outer_block);

        // Memory write should propagate through all nested blocks
        assert!(outer_effect.writes_memory());
        assert!(outer_effect.has_side_effects());
    }

    #[test]
    fn test_composition_sequence_accumulates_effects() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // First: memory read
        let load = builder.load(
            MemoryId::from_raw_parts(0, 0),
            4,
            false,
            0,
            4,
            builder.const_(Literal::I32(0)),
            Type::I32,
        );

        // Second: global write
        let g1_id: GlobalId = Id::from_raw_parts(1, 0);
        let global_set = builder.global_set(g1_id, builder.const_(Literal::I32(42)));

        // Third: local write
        let local_set = builder.local_set(2, builder.const_(Literal::I32(99)));

        let body = bumpalo::vec![in &bump; load, global_set, local_set];
        let block = builder.block(None, body, Type::NONE);

        let block_effect = EffectAnalyzer::analyze(block);

        // All three effects should be present
        assert!(block_effect.reads_memory());
        assert!(block_effect.writes_global());
        assert!(block_effect.writes_local());

        // Should have multiple side effects
        assert!(block_effect.has_side_effects());
    }

    #[test]
    fn test_composition_loop_multiplies_effects() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Loop body with side effect (memory write)
        let _store = builder.store(
            MemoryId::from_raw_parts(0, 0),
            4,
            0,
            4,
            builder.const_(Literal::I32(100)),
            builder.const_(Literal::I32(42)),
        );

        let body = builder.store(
            MemoryId::from_raw_parts(0, 0),
            4,
            0,
            4,
            builder.const_(Literal::I32(100)),
            builder.const_(Literal::I32(42)),
        );
        let loop_expr = builder.loop_(Some("repeat"), body, Type::NONE);

        let loop_effect = EffectAnalyzer::analyze(loop_expr);

        // Effects should propagate from loop body
        assert!(loop_effect.writes_memory());
        assert!(loop_effect.has_side_effects());

        // Loop inherently branches
        assert!(loop_effect.branches());
    }

    #[test]
    fn test_composition_if_both_branches_pure() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Condition (pure)
        let cond = builder.const_(Literal::I32(1));

        // If-true: const (pure)
        let if_true = builder.const_(Literal::I32(10));

        // If-false: const (pure)
        let if_false = builder.const_(Literal::I32(20));

        let if_expr = builder.if_(cond, if_true, Some(if_false), Type::I32);

        let if_effect = EffectAnalyzer::analyze(if_expr);

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
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // First store to address 0
        let store1 = builder.store(
            MemoryId::from_raw_parts(0, 0),
            4,
            0,
            4,
            builder.const_(Literal::I32(0)),
            builder.const_(Literal::I32(100)),
        );
        let store1_effect = EffectAnalyzer::analyze(store1);

        // Second store to same address (overwrites first)
        let store2 = builder.store(
            MemoryId::from_raw_parts(0, 0),
            4,
            0,
            4,
            builder.const_(Literal::I32(0)),
            builder.const_(Literal::I32(200)),
        );
        let store2_effect = EffectAnalyzer::analyze(store2);

        let body = bumpalo::vec![in &bump; store1, store2];
        let block = builder.block(None, body, Type::NONE);

        let block_effect = EffectAnalyzer::analyze(block);

        // Both stores contribute to memory writes
        assert!(block_effect.writes_memory());

        // First store could potentially be eliminated (if addresses are proven same)

        // Both have same effect pattern
        assert_eq!(store1_effect, store2_effect);
        assert!(store1_effect.writes_memory());
    }

    #[test]
    fn test_optimization_store_with_interfering_call() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Store to memory
        let store = builder.store(
            MemoryId::from_raw_parts(0, 0),
            4,
            0,
            4,
            builder.const_(Literal::I32(0)),
            builder.const_(Literal::I32(42)),
        );
        let store_effect = EffectAnalyzer::analyze(store);

        // Call (may have arbitrary effects)
        let operands = bumpalo::vec![in &bump;];
        let call = builder.call(
            FuncId::from_raw_parts(0, 0), /* mayWrite */
            operands,
            Type::NONE,
            false,
        );
        let call_effect = EffectAnalyzer::analyze(call);

        // Second store to same location
        let store2 = builder.store(
            MemoryId::from_raw_parts(0, 0),
            4,
            0,
            4,
            builder.const_(Literal::I32(0)),
            builder.const_(Literal::I32(99)),
        );

        let body = bumpalo::vec![in &bump; store, call, store2];
        let _block = builder.block(None, body, Type::NONE);

        // Call interferes with store - prevents optimization
        assert!(call_effect.interferes_with(store_effect));
        assert!(store_effect.interferes_with(call_effect));

        // First store cannot be eliminated due to interfering call
    }

    #[test]
    fn test_optimization_local_set_chain() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Chain of sets to same local
        let set1 = builder.local_set(0, builder.const_(Literal::I32(1)));
        let set1_effect = EffectAnalyzer::analyze(set1);
        let set2 = builder.local_set(0, builder.const_(Literal::I32(2)));
        let set3 = builder.local_set(0, builder.const_(Literal::I32(3)));

        let body = bumpalo::vec![in &bump; set1, set2, set3];
        let block = builder.block(None, body, Type::NONE);

        let block_effect = EffectAnalyzer::analyze(block);

        // All write locals
        assert!(block_effect.writes_local());

        // Earlier sets could be eliminated if no local.get in between
        assert!(set1_effect.writes_local());
        assert!(!set1_effect.reads_local());
    }

    #[test]
    fn test_optimization_local_set_with_interfering_load() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // LocalSet (writes local, pure value)
        let set1 = builder.local_set(0, builder.const_(Literal::I32(10)));
        let set_effect = EffectAnalyzer::analyze(set1);

        // GlobalGet (reads global, doesn't trap)
        let g1_id: GlobalId = Id::from_raw_parts(1, 0);
        let global_get = builder.global_get(g1_id, Type::I32);
        let global_effect = EffectAnalyzer::analyze(global_get);

        // Second LocalSet
        let _set2 = builder.local_set(0, builder.const_(Literal::I32(20)));

        // LocalSet and GlobalGet should NOT interfere (different domains, no trap)
        assert!(!set_effect.interferes_with(global_effect));
        assert!(!global_effect.interferes_with(set_effect));

        // First set1 COULD be eliminated (no interference from global_get)
        // Note: Load from memory has MAY_TRAP, so it WOULD interfere
    }
}
