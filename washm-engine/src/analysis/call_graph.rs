use crate::ir::expression::{ExprRef, ExpressionKind};
use crate::ir::module::Module;
use crate::ir::visitor::ReadOnlyVisitor;
use std::collections::{HashMap, HashSet};

/// Call Graph representation
/// Tracks direct calls between functions.
pub struct CallGraph {
    /// Direct calls from each function (Caller -> Callees)
    pub calls: HashMap<String, HashSet<String>>,

    /// Functions that call this function (Callee -> Callers)
    pub callers: HashMap<String, HashSet<String>>,
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
        for (_, func) in &module.functions {
            graph.calls.insert(func.name.clone(), HashSet::new());
            graph.callers.insert(func.name.clone(), HashSet::new());
        }

        for (_, func) in &module.functions {
            if let Some(body) = func.body() {
                let caller_name = func.name.clone();
                let mut visitor = CallGraphBuilder {
                    caller: caller_name,
                    graph: &mut graph,
                    module,
                };
                visitor.visit(body);
            }
        }
        graph
    }

    /// Get all functions directly called by `func`
    pub fn get_callees(&self, func: &str) -> Option<&HashSet<String>> {
        self.calls.get(func)
    }

    /// Get all functions that directly call `func`
    pub fn get_callers(&self, func: &str) -> Option<&HashSet<String>> {
        self.callers.get(func)
    }

    /// Propagate a property backwards from callees to callers.
    /// `initial` is the set of functions that initially have the property.
    /// `can_have` is a filter function to check if a function can ever have the property.
    pub fn propagate_back<F>(&self, initial: HashSet<String>, mut can_have: F) -> HashSet<String>
    where
        F: FnMut(&str) -> bool,
    {
        let mut result = initial;
        let mut worklist: Vec<String> = result.iter().cloned().collect();

        while let Some(curr) = worklist.pop() {
            if let Some(callers) = self.get_callers(&curr) {
                for caller in callers {
                    if !result.contains(caller) && can_have(caller) {
                        result.insert(caller.clone());
                        worklist.push(caller.clone());
                    }
                }
            }
        }

        result
    }
}

struct CallGraphBuilder<'a, 'm> {
    caller: String,
    graph: &'a mut CallGraph,
    module: &'m Module<'a>,
}

impl<'a, 'b, 'm> ReadOnlyVisitor<'a> for CallGraphBuilder<'b, 'm> {
    fn visit_expression(&mut self, expr: ExprRef<'a>) {
        if let ExpressionKind::Call { target, .. } = &expr.kind {
            // Record call
            let target_name = self.module.functions[*target].name.clone();

            self.graph
                .calls
                .entry(self.caller.clone())
                .or_default()
                .insert(target_name.clone());

            self.graph
                .callers
                .entry(target_name)
                .or_default()
                .insert(self.caller.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::IrBuilder;
    use crate::ir::module::Function;
    use crate::ir::Type;
    use bumpalo::Bump;

    #[test]
    fn test_call_graph_construction() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Create function "callee"
        let nop = builder.nop();
        let callee = Function::new(
            "callee".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(nop),
        );

        // Create function "caller" calling "callee"
        let call = builder.call(
            "callee",
            bumpalo::collections::Vec::new_in(&bump),
            Type::NONE,
            false,
        );
        let caller = Function::new(
            "caller".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(call),
        );

        let mut module = Module::new(&bump);
        module.add_function(callee);
        module.add_function(caller);

        let graph = CallGraph::build(&module);

        // Check "caller" -> "callee" edge
        let callees = graph.get_callees("caller").unwrap();
        assert!(callees.contains("callee"));
        assert_eq!(callees.len(), 1);

        // Check "callee" <- "caller" edge
        let callers = graph.get_callers("callee").unwrap();
        assert!(callers.contains("caller"));
        assert_eq!(callers.len(), 1);
    }
}
