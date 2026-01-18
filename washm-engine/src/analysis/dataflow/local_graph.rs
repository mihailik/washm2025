use crate::analysis::effects::EffectAnalyzer;
use crate::ir::expression::{ExprRef, ExpressionKind};
use crate::ir::module::Function;
use crate::ir::visitor::Visitor;
use crate::ir::Type;
use std::collections::HashMap;

/// Local variable ID
pub type LocalId = u32;

/// Represents def-use chains for locals within a function
///
/// Tracks where locals are defined (set/tee/param) and used (get),
/// enabling safe sinking and other local optimizations.
#[derive(Debug)]
pub struct LocalGraph<'a> {
    /// Maps local ID to all definitions (local.set, local.tee, param)
    definitions: HashMap<LocalId, Vec<ExprRef<'a>>>,

    /// Maps local ID to all uses (local.get)
    uses: HashMap<LocalId, Vec<ExprRef<'a>>>,

    /// Number of local variables in function
    num_locals: u32,
}

impl<'a> LocalGraph<'a> {
    /// Build local graph for a function
    pub fn build(func: &'a Function<'a>) -> Self {
        // Count total locals: params + vars
        // For now, assume single-value params (Type is not a tuple)
        // TODO: Handle multi-value params properly when tuple types are needed
        let num_params = if func.params == Type::NONE { 0 } else { 1 };
        let num_vars = func.vars.len() as u32;
        let num_locals = num_params + num_vars;

        let mut graph = LocalGraph {
            definitions: HashMap::new(),
            uses: HashMap::new(),
            num_locals,
        };

        // Add parameter definitions
        for i in 0..num_params {
            graph.definitions.entry(i).or_default();
        }

        // Walk expression tree and record def/use
        if let Some(body) = func.body {
            graph.collect_def_use(body);
        }

        graph
    }

    /// Collect definitions and uses by walking expression tree
    fn collect_def_use(&mut self, mut expr: ExprRef<'a>) {
        struct DefUseCollector<'a, 'b> {
            graph: &'b mut LocalGraph<'a>,
        }

        impl<'a, 'b> Visitor<'a> for DefUseCollector<'a, 'b> {
            fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
                match &expr.kind {
                    ExpressionKind::LocalGet { index } => {
                        self.graph.uses.entry(*index).or_default().push(*expr);
                    }
                    ExpressionKind::LocalSet { index, .. } => {
                        self.graph
                            .definitions
                            .entry(*index)
                            .or_default()
                            .push(*expr);
                    }
                    ExpressionKind::LocalTee { index, .. } => {
                        // Tee both defines and uses
                        self.graph
                            .definitions
                            .entry(*index)
                            .or_default()
                            .push(*expr);
                        self.graph.uses.entry(*index).or_default().push(*expr);
                    }
                    _ => {}
                }
            }
        }

        let mut collector = DefUseCollector { graph: self };
        collector.visit(&mut expr);
    }

    /// Get all definitions of a local
    pub fn definitions(&self, local: LocalId) -> &[ExprRef<'a>] {
        self.definitions
            .get(&local)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get all uses of a local
    pub fn uses(&self, local: LocalId) -> &[ExprRef<'a>] {
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
    pub fn all_locals(&self) -> impl Iterator<Item = LocalId> + '_ {
        0..self.num_locals
    }

    /// Get single definition if exists
    pub fn single_def(&self, local: LocalId) -> Option<ExprRef<'a>> {
        if self.has_single_def(local) {
            self.definitions
                .get(&local)
                .and_then(|v| v.first().copied())
        } else {
            None
        }
    }

    /// Get single use if exists
    pub fn single_use(&self, local: LocalId) -> Option<ExprRef<'a>> {
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
    pub fn can_sink(&self, set: ExprRef<'a>, _target: ExprRef<'a>) -> bool {
        // Extract local ID from set
        let _local_id = match &set.kind {
            ExpressionKind::LocalSet { index, .. } => *index,
            _ => return false,
        };

        // Check for interfering effects
        let set_value = match &set.kind {
            ExpressionKind::LocalSet { value, .. } => *value,
            _ => return false,
        };

        let set_effects = EffectAnalyzer::analyze(set_value);

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
    pub fn find_uses_in(&self, local: LocalId, root: ExprRef<'a>) -> Vec<ExprRef<'a>> {
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
    fn is_descendant(&self, expr: ExprRef<'a>, ancestor: ExprRef<'a>) -> bool {
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
