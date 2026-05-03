//! Effect analysis for WebAssembly expressions.
//!
//! This module provides the foundational types for reasoning about side effects
//! in WebAssembly expressions. Effects are used by optimization passes to
//! determine whether it is safe to reorder, eliminate, or move instructions.
//!
//! ## Design Philosophy
//!
//! In Rust, we leverage the type system to make effect analysis more precise
//! and harder to misuse than the C++ counterpart. Effects are represented as
//! bitflags, allowing efficient composition and querying.
//!
//! ## Usage
//!
//! ```rust
//! use washm::analysis::effects::{Effect, EffectAnalyzer};
//! use washm::ir::expression::{IrBuilder, ExpressionId};
//! use washm::ir::{Literal, Type};
//! use washm::collections::IdArena;
//!
//! let mut arena = IdArena::new();
//! let mut builder = IrBuilder::new(&mut arena);
//! let expr = builder.unreachable();
//!
//! let effect = EffectAnalyzer::analyze(expr, &arena);
//! assert!(effect.may_trap());
//! assert!(effect.transfers_control());
//! ```

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
        const SIDE_EFFECTS = Self::WRITES.bits()
            | Self::CALLS.bits()
            | Self::TRAPS.bits()
            | Self::MAY_TRAP.bits()
            | Self::THROWS.bits()
            | Self::BRANCHES.bits()
            | Self::GROWS.bits();
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
        let mut effect = match &expr.kind {
            ExpressionKind::Nop | ExpressionKind::Const(_) | ExpressionKind::LocalGet { .. } => {
                Effect::NONE
            }

            ExpressionKind::Unreachable => Effect::TRAPS,

            ExpressionKind::Unary { op, .. } => {
                if op.may_trap() {
                    Effect::MAY_TRAP
                } else {
                    Effect::NONE
                }
            }

            ExpressionKind::Binary { op, .. } => {
                if op.may_trap() {
                    Effect::MAY_TRAP
                } else {
                    Effect::NONE
                }
            }

            ExpressionKind::LocalSet { .. } | ExpressionKind::LocalTee { .. } => {
                Effect::LOCAL_WRITE
            }

            ExpressionKind::GlobalGet { .. } => Effect::GLOBAL_READ,

            ExpressionKind::GlobalSet { .. } => Effect::GLOBAL_WRITE,

            ExpressionKind::Load { .. } => Effect::MEMORY_READ | Effect::MAY_TRAP,

            ExpressionKind::Store { .. } => Effect::MEMORY_WRITE | Effect::MAY_TRAP,

            ExpressionKind::Call { .. } => Effect::CALLS,

            ExpressionKind::CallIndirect { .. } => {
                Effect::CALLS | Effect::TABLE_ACCESS | Effect::MAY_TRAP
            }

            ExpressionKind::If { .. }
            | ExpressionKind::Loop { .. }
            | ExpressionKind::Break { .. }
            | ExpressionKind::Return { .. }
            | ExpressionKind::Switch { .. }
            | ExpressionKind::BrOn { .. } => Effect::BRANCHES,

            ExpressionKind::Drop { .. } | ExpressionKind::Select { .. } => Effect::NONE,

            ExpressionKind::MemorySize { .. } => Effect::MEMORY_READ,

            ExpressionKind::MemoryGrow { .. } => Effect::GROWS | Effect::NONDETERMINISTIC,

            ExpressionKind::AtomicRMW { .. }
            | ExpressionKind::AtomicCmpxchg { .. }
            | ExpressionKind::AtomicNotify { .. } => {
                Effect::MEMORY_READ | Effect::MEMORY_WRITE | Effect::MAY_TRAP
            }

            ExpressionKind::AtomicWait { .. } => {
                Effect::MEMORY_READ | Effect::CALLS | Effect::MAY_TRAP
            }

            ExpressionKind::AtomicFence => Effect::MEMORY_WRITE,

            ExpressionKind::SIMDExtract { .. }
            | ExpressionKind::SIMDReplace { .. }
            | ExpressionKind::SIMDShuffle { .. }
            | ExpressionKind::SIMDTernary { .. }
            | ExpressionKind::SIMDShift { .. }
            | ExpressionKind::SIMDLoad { .. } => Effect::MAY_TRAP,

            ExpressionKind::SIMDLoadStoreLane { .. } => {
                Effect::MEMORY_READ | Effect::MEMORY_WRITE | Effect::MAY_TRAP
            }

            ExpressionKind::MemoryInit { .. }
            | ExpressionKind::MemoryCopy { .. }
            | ExpressionKind::MemoryFill { .. } => {
                Effect::MEMORY_READ | Effect::MEMORY_WRITE | Effect::MAY_TRAP
            }

            ExpressionKind::DataDrop { .. } | ExpressionKind::ElemDrop { .. } => {
                Effect::MEMORY_WRITE
            }

            ExpressionKind::TableGet { .. } | ExpressionKind::TableSize { .. } => {
                Effect::TABLE_ACCESS
            }

            ExpressionKind::TableSet { .. }
            | ExpressionKind::TableGrow { .. }
            | ExpressionKind::TableFill { .. }
            | ExpressionKind::TableCopy { .. }
            | ExpressionKind::TableInit { .. } => {
                Effect::TABLE_ACCESS | Effect::WRITES | Effect::MAY_TRAP
            }

            ExpressionKind::RefCast { .. } | ExpressionKind::RefAs { .. } => Effect::MAY_TRAP,

            ExpressionKind::StructNew { .. }
            | ExpressionKind::StructGet { .. }
            | ExpressionKind::StructSet { .. }
            | ExpressionKind::ArrayNew { .. }
            | ExpressionKind::ArrayGet { .. }
            | ExpressionKind::ArraySet { .. }
            | ExpressionKind::ArrayLen { .. } => {
                Effect::HEAPS_READ | Effect::HEAPS_WRITE | Effect::MAY_TRAP
            }

            ExpressionKind::Try { .. }
            | ExpressionKind::Throw { .. }
            | ExpressionKind::Rethrow { .. } => Effect::THROWS,

            _ => Effect::NONE,
        };

        expr.kind.for_each_child(|child_id| {
            effect |= Self::analyze(child_id, arena);
        });

        effect
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
    fn test_analyze_simple() {
        use crate::ir::expression::IrBuilder;
        use crate::ir::Literal;
        let mut arena = IdArena::new();
        let c;
        let u;
        {
            let mut builder = IrBuilder::new(&mut arena);
            c = builder.const_(Literal::I32(42));
            u = builder.unreachable();
        }

        assert!(EffectAnalyzer::analyze(c, &arena).is_pure());
        assert!(EffectAnalyzer::analyze(u, &arena).traps());
    }

    #[test]
    fn test_analyze_nested() {
        use crate::ir::expression::IrBuilder;
        use crate::ir::Literal;
        use crate::ir::Type;
        let mut arena = IdArena::new();
        let if_expr;
        {
            let mut builder = IrBuilder::new(&mut arena);
            let c = builder.const_(Literal::I32(42));
            let u = builder.unreachable();
            if_expr = builder.if_(None::<&str>, c, u, None, Type::NONE);
        }

        let effect = EffectAnalyzer::analyze(if_expr, &arena);
        assert!(effect.branches());
        assert!(effect.traps());
    }
}
