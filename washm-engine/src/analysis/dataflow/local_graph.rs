use crate::analysis::effects::EffectAnalyzer;
use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::module::{Function, LocalId};
use crate::ir::visitor::ReadOnlyVisitor;
use std::collections::HashMap;

/// Represents def-use chains for locals within a function
///
/// Tracks where locals are defined (set/tee/param) and used (get),
/// enabling safe sinking and other local optimizations.
#[derive(Debug)]
pub struct LocalGraph {
    /// Maps local ID to all definitions (local.set, local.tee, param)
    definitions: HashMap<LocalId, Vec<ExpressionId>>,

    /// Maps local ID to all uses (local.get)
    uses: HashMap<LocalId, Vec<ExpressionId>>,

    /// Number of local variables in function
    num_locals: usize,
}

impl LocalGraph {
    /// Build local graph for a function
    pub fn build(func: &Function, arena: &IdArena<ExpressionMarker, Expression>) -> Self {
        let num_locals = if func.is_import() {
            func.num_params()
        } else {
            func.locals().len()
        };

        let mut graph = LocalGraph {
            definitions: HashMap::new(),
            uses: HashMap::new(),
            num_locals,
        };

        // Add parameter definitions
        for id in func.param_ids() {
            graph.definitions.entry(*id).or_default();
        }

        // Walk expression tree and record def/use
        if let Some(body) = func.body() {
            graph.collect_def_use(body, arena);
        }

        graph
    }

    /// Collect definitions and uses by walking expression tree
    fn collect_def_use(
        &mut self,
        body: ExpressionId,
        arena: &IdArena<ExpressionMarker, Expression>,
    ) {
        struct DefUseCollector<'a> {
            graph: &'a mut LocalGraph,
        }

        impl<'a> ReadOnlyVisitor for DefUseCollector<'a> {
            fn visit_expression(
                &mut self,
                expr: ExpressionId,
                arena: &IdArena<ExpressionMarker, Expression>,
            ) {
                match &arena[expr].kind {
                    ExpressionKind::LocalGet { id } => {
                        self.graph.uses.entry(*id).or_default().push(expr);
                    }
                    ExpressionKind::LocalSet { id, .. } => {
                        self.graph.definitions.entry(*id).or_default().push(expr);
                    }
                    ExpressionKind::LocalTee { id, .. } => {
                        // Tee both defines and uses
                        self.graph.definitions.entry(*id).or_default().push(expr);
                        self.graph.uses.entry(*id).or_default().push(expr);
                    }
                    _ => {}
                }
            }
        }

        let mut collector = DefUseCollector { graph: self };
        collector.visit(body, arena);
    }

    /// Get all definitions of a local
    pub fn definitions(&self, local: LocalId) -> &[ExpressionId] {
        self.definitions
            .get(&local)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get all uses of a local
    pub fn uses(&self, local: LocalId) -> &[ExpressionId] {
        self.uses.get(&local).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Count number of uses of a local
    pub fn use_count(&self, local: LocalId) -> usize {
        self.uses.get(&local).map(|v| v.len()).unwrap_or(0)
    }

    /// Count number of definitions of a local
    pub fn def_count(&self, local: LocalId) -> usize {
        self.definitions.get(&local).map(|v| v.len()).unwrap_or(0)
    }

    /// Check if a local is never used
    pub fn is_unused(&self, local: LocalId) -> bool {
        self.use_count(local) == 0
    }

    /// Check if a local has a single definition
    pub fn has_single_def(&self, local: LocalId) -> bool {
        self.def_count(local) == 1
    }

    /// Check if a local has a single use
    pub fn has_single_use(&self, local: LocalId) -> bool {
        self.use_count(local) == 1
    }

    /// Get all locals defined in function
    pub fn all_locals(&self) -> impl Iterator<Item = crate::ir::LocalId> + '_ {
        (0..self.num_locals).map(|i| crate::ir::LocalId::from_u32(i as u32))
    }

    /// Get single definition if exists
    pub fn single_def(&self, local: LocalId) -> Option<ExpressionId> {
        if self.has_single_def(local) {
            self.definitions
                .get(&local)
                .and_then(|v| v.first().copied())
        } else {
            None
        }
    }

    /// Get single use if exists
    pub fn single_use(&self, local: LocalId) -> Option<ExpressionId> {
        if self.has_single_use(local) {
            self.uses.get(&local).and_then(|v| v.first().copied())
        } else {
            None
        }
    }

    /// Check if we can safely sink a local.set to a target location
    ///
    /// Requirements:
    /// - No interfering effects between set and target
    /// - No other definitions of same local between set and target
    /// - All uses of the local happen after target (or are dominated by target)
    pub fn can_sink(
        &self,
        set: ExpressionId,
        _target: ExpressionId,
        arena: &IdArena<ExpressionMarker, Expression>,
    ) -> bool {
        // Extract local ID from set
        let _local_id = match &arena[set].kind {
            ExpressionKind::LocalSet { id, .. } => *id,
            _ => return false,
        };

        // Check for interfering effects
        let set_value = match &arena[set].kind {
            ExpressionKind::LocalSet { value, .. } => *value,
            _ => return false,
        };

        let set_effects = EffectAnalyzer::analyze(set_value, arena);

        // If set value has side effects, sinking is risky
        if set_effects.has_side_effects() {
            return false;
        }

        // TODO: More sophisticated analysis needed:
        // - Check expressions between set and target for interference
        // - Verify dominance relationships
        // - Check for other definitions

        true
    }

    /// Find all uses of a local within a subtree
    pub fn find_uses_in(&self, local: LocalId, root: ExpressionId) -> Vec<ExpressionId> {
        let mut result = Vec::new();
        let uses = self.uses(local);

        // Simple approach: check if each use is within subtree rooted at root
        // TODO: More efficient tree traversal
        for &use_expr in uses {
            if self.is_descendant(use_expr, root) {
                result.push(use_expr);
            }
        }

        result
    }

    /// Check if expr is a descendant of ancestor
    fn is_descendant(&self, expr: ExpressionId, ancestor: ExpressionId) -> bool {
        if expr == ancestor {
            return true;
        }

        // Walk up from expr to see if we reach ancestor
        // For now, simplified version - would need parent pointers for full implementation
        // TODO: Add parent tracking to enable this
        false
    }
}

#[cfg(test)]
mod tests {
    // TODO: Add tests once Expression builder API is finalized
    // Tests disabled temporarily to unblock infrastructure implementation
}
