use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::module::{FuncId, Module};
use crate::ir::visitor::ReadOnlyVisitor;
use std::collections::{HashMap, HashSet};

/// Call Graph representation
/// Tracks direct calls between functions.
pub struct CallGraph {
    /// Direct calls from each function (Caller -> Callees)
    pub calls: HashMap<FuncId, HashSet<FuncId>>,

    /// Functions that call this function (Callee -> Callers)
    pub callers: HashMap<FuncId, HashSet<FuncId>>,
}

impl Default for CallGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl CallGraph {
    pub fn new() -> Self {
        Self {
            calls: HashMap::new(),
            callers: HashMap::new(),
        }
    }

    pub fn build(module: &Module) -> Self {
        let mut graph = Self::new();

        // Initialize nodes for all functions
        for (id, _) in module.functions.iter() {
            graph.calls.insert(id, HashSet::new());
            graph.callers.insert(id, HashSet::new());
        }

        for (id, func) in module.functions.iter() {
            if let Some(body) = func.body() {
                let mut visitor = CallGraphBuilder {
                    caller: id,
                    graph: &mut graph,
                };
                visitor.visit(body, &module.expressions);
            }
        }
        graph
    }

    /// Get all functions directly called by `func`
    pub fn get_callees(&self, func: FuncId) -> Option<&HashSet<FuncId>> {
        self.calls.get(&func)
    }

    /// Get all functions that directly call `func`
    pub fn get_callers(&self, func: FuncId) -> Option<&HashSet<FuncId>> {
        self.callers.get(&func)
    }

    /// Propagate a property backwards from callees to callers.
    /// `initial` is the set of functions that initially have the property.
    /// `can_have` is a filter function to check if a function can ever have the property.
    pub fn propagate_back<F>(&self, initial: HashSet<FuncId>, mut can_have: F) -> HashSet<FuncId>
    where
        F: FnMut(FuncId) -> bool,
    {
        let mut result = initial;
        let mut worklist: Vec<FuncId> = result.iter().cloned().collect();

        while let Some(curr) = worklist.pop() {
            if let Some(callers) = self.get_callers(curr) {
                for &caller in callers {
                    if !result.contains(&caller) && can_have(caller) {
                        result.insert(caller);
                        worklist.push(caller);
                    }
                }
            }
        }

        result
    }
}

struct CallGraphBuilder<'a> {
    caller: FuncId,
    graph: &'a mut CallGraph,
}

impl<'a> ReadOnlyVisitor for CallGraphBuilder<'a> {
    fn visit_expression(
        &mut self,
        expr: ExpressionId,
        arena: &IdArena<ExpressionMarker, Expression>,
    ) {
        match &arena[expr].kind {
            ExpressionKind::Call { target, .. } => {
                self.graph
                    .calls
                    .get_mut(&self.caller)
                    .unwrap()
                    .insert(*target);

                self.graph
                    .callers
                    .entry(*target)
                    .or_default()
                    .insert(self.caller);
            }
            ExpressionKind::CallIndirect { .. } => {
                // TODO: Indirect calls handling
            }
            _ => {}
        }
        self.visit_children(expr, arena);
    }
}
