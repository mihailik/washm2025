use crate::analysis::call_graph::CallGraph;
use crate::ir::expression::{ExprRef, ExpressionKind};
use crate::ir::module::Module;
use crate::ir::{Literal, GlobalId};
use std::collections::{HashMap, HashSet};

pub type FunctionId = usize;

pub struct GlobalAnalysis {
    /// Globals that are immutable (imported or constant) or effectively constant (written once at init)
    pub constant_globals: HashSet<GlobalId>,

    /// Known values for constant globals
    pub global_values: HashMap<GlobalId, Literal>,

    /// Which globals are read
    pub read_globals: HashSet<GlobalId>,

    /// Which globals are written
    pub written_globals: HashSet<GlobalId>,

    /// How many times each global is read
    pub read_counts: HashMap<GlobalId, usize>,

    /// How many times each global is written
    pub write_counts: HashMap<GlobalId, usize>,

    /// Globals that are written a value different from their initial value
    pub non_init_written: HashSet<GlobalId>,

    /// Which functions are reachable from exports
    pub reachable_functions: HashSet<FunctionId>,
}

impl GlobalAnalysis {
    pub fn new() -> Self {
        Self {
            constant_globals: HashSet::new(),
            global_values: HashMap::new(),
            read_globals: HashSet::new(),
            written_globals: HashSet::new(),
            read_counts: HashMap::new(),
            write_counts: HashMap::new(),
            non_init_written: HashSet::new(),
            reachable_functions: HashSet::new(),
        }
    }

    pub fn analyze(module: &Module, call_graph: &CallGraph) -> Self {
        let mut analysis = Self::new();

        analysis.analyze_usage(module);
        analysis.analyze_globals(module);
        analysis.analyze_reachability(module, call_graph);

        analysis
    }

    fn analyze_usage(&mut self, module: &Module) {
        for func in &module.functions {
            if let Some(body) = func.body {
                self.visit_usage(module, body);
            }
        }

        // Visit global initializers
        for (_, global) in module.globals.iter() {
            if let crate::ir::module::GlobalKind::Local { init } = &global.kind {
                self.visit_usage(module, *init);
            }
        }

        // Mark exports as read/written
        for export in &module.exports {
            if let crate::ir::module::ExportKind::Global(id) = export.kind {
                self.read_globals.insert(id);
                self.written_globals.insert(id);
                *self.write_counts.entry(id).or_insert(0) += 1;
                *self.read_counts.entry(id).or_insert(0) += 1;
            }
        }
    }

    fn analyze_globals(&mut self, module: &Module) {
        // We perform multiple passes to resolve globals that depend on other constant globals in their initializers
        let mut changed = true;
        while changed {
            changed = false;

            for (id, global) in module.globals.iter() {
                let gid = id.cast();
                if self.constant_globals.contains(&gid) && self.global_values.contains_key(&gid) {
                    continue;
                }

                let mut is_effectively_constant = false;
                if !global.mutable {
                    is_effectively_constant = true;
                } else if !self.written_globals.contains(&gid)
                    || !self.non_init_written.contains(&gid)
                {
                    is_effectively_constant = true;
                }

                if is_effectively_constant {
                    self.constant_globals.insert(gid);

                    // Try to resolve the value
                    if let crate::ir::module::GlobalKind::Local { init } = &global.kind {
                        if let ExpressionKind::Const(value) = &init.kind {
                            if !self.global_values.contains_key(&gid) {
                                self.global_values.insert(gid, value.clone());
                                changed = true;
                            }
                        } else if let ExpressionKind::GlobalGet { id: other_id } = &init.kind {
                            if let Some(value) = self.global_values.get(other_id) {
                                let val = value.clone();
                                if !self.global_values.get(&gid).map_or(false, |v| v == &val) {
                                    self.global_values.insert(gid, val);
                                    changed = true;
                                }
                            }
                        }
                    } else if let crate::ir::module::GlobalKind::Import { .. } = &global.kind {
                        if !global.mutable {
                            self.constant_globals.insert(gid);
                        }
                    }
                }
            }
        }
    }

    fn visit_usage(&mut self, module: &Module, expr: ExprRef) {
        match &expr.kind {
            ExpressionKind::GlobalGet { id } => {
                let id = *id;
                self.read_globals.insert(id);
                *self.read_counts.entry(id).or_insert(0) += 1;
            }
            ExpressionKind::GlobalSet { id, value } => {
                let id = *id;
                self.written_globals.insert(id);
                *self.write_counts.entry(id).or_insert(0) += 1;

                // Check if we are writing a value different from the initializer
                if let ExpressionKind::Const(lit) = &value.kind {
                    if let Some(global) = module.globals.get(id.cast()) {
                        if let crate::ir::module::GlobalKind::Local { init } = &global.kind {
                            if let ExpressionKind::Const(init_lit) = &init.kind {
                                if lit != init_lit {
                                    self.non_init_written.insert(id);
                                }
                            } else {
                                // Non-const initializer, any write is non-init (effectively)
                                self.non_init_written.insert(id);
                            }
                        }
                    }
                } else {
                    // Non-const value being set
                    self.non_init_written.insert(id);
                }

                self.visit_usage(module, *value);
            }
            ExpressionKind::Block { list, .. } => {
                for child in list.iter() {
                    self.visit_usage(module, *child);
                }
            }
            ExpressionKind::Loop { body, .. } => self.visit_usage(module, *body),
            ExpressionKind::If {
                condition,
                if_true,
                if_false,
                ..
            } => {
                self.visit_usage(module, *condition);
                self.visit_usage(module, *if_true);
                if let Some(e) = if_false {
                    self.visit_usage(module, *e);
                }
            }
            ExpressionKind::Binary { left, right, .. } => {
                self.visit_usage(module, *left);
                self.visit_usage(module, *right);
            }
            ExpressionKind::Unary { value, .. } => self.visit_usage(module, *value),
            ExpressionKind::Const(_) | ExpressionKind::Nop | ExpressionKind::Unreachable => {}
            ExpressionKind::Call { operands, .. }
            | ExpressionKind::CallIndirect { operands, .. } => {
                for arg in operands {
                    self.visit_usage(module, *arg);
                }
                if let ExpressionKind::CallIndirect { target, .. } = &expr.kind {
                    self.visit_usage(module, *target);
                }
            }
            ExpressionKind::LocalGet { .. } => {}
            ExpressionKind::LocalSet { value, .. } | ExpressionKind::LocalTee { value, .. } => {
                self.visit_usage(module, *value);
            }
            ExpressionKind::Drop { value } => self.visit_usage(module, *value),
            ExpressionKind::Select {
                condition,
                if_true,
                if_false,
            } => {
                self.visit_usage(module, *condition);
                self.visit_usage(module, *if_true);
                self.visit_usage(module, *if_false);
            }
            ExpressionKind::Load { ptr, .. } => self.visit_usage(module, *ptr),
            ExpressionKind::Store { ptr, value, .. } => {
                self.visit_usage(module, *ptr);
                self.visit_usage(module, *value);
            }
            ExpressionKind::Return { value } => {
                if let Some(v) = value {
                    self.visit_usage(module, *v);
                }
            }
            ExpressionKind::Switch {
                condition, value, ..
            } => {
                self.visit_usage(module, *condition);
                if let Some(v) = value {
                    self.visit_usage(module, *v);
                }
            }
            _ => {
                // Fallback for other expressions if any
            }
        }
    }

    fn analyze_reachability(&mut self, module: &Module, call_graph: &CallGraph) {
        // Map function name to index
        let mut name_to_index: HashMap<&str, usize> = HashMap::new();
        for (i, func) in module.functions.iter().enumerate() {
            name_to_index.insert(&func.name, i);
        }

        // Roots: exports, start function, table elements?
        let mut roots = HashSet::new();

        // 1. Exports
        for export in &module.exports {
            if let crate::ir::module::ExportKind::Function(idx) = export.kind {
                roots.insert(idx as usize);
            }
        }

        // Compute reachability
        let mut visited = HashSet::new();
        let mut worklist: Vec<usize> = roots.into_iter().collect();

        while let Some(func_id) = worklist.pop() {
            if !visited.contains(&func_id) {
                visited.insert(func_id);
                self.reachable_functions.insert(func_id);

                // Get function name to lookup in CallGraph
                if let Some(func) = module.functions.get(func_id) {
                    if let Some(callees) = call_graph.calls.get(&func.name) {
                        for callee_name in callees {
                            if let Some(&callee_idx) = name_to_index.get(callee_name.as_str()) {
                                if !visited.contains(&callee_idx) {
                                    worklist.push(callee_idx);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::IrBuilder;
    use crate::ir::module::{Function, Module};
    use crate::ir::Type;
    use bumpalo::Bump;

    #[test]
    fn test_reachability() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Func 0: Exported, calls Func 1
        let call1 = builder.call(
            "func1",
            bumpalo::collections::Vec::new_in(&bump),
            Type::NONE,
            false,
        );
        let func0 = Function::new(
            "func0".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(call1),
        );

        // Func 1: Called by Func 0, calls Func 2
        let call2 = builder.call(
            "func2",
            bumpalo::collections::Vec::new_in(&bump),
            Type::NONE,
            false,
        );
        let func1 = Function::new(
            "func1".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(call2),
        );

        // Func 2: Called by Func 1
        let nop = builder.nop();
        let func2_def = Function::new(
            "func2".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(nop),
        );

        // Func 3: Unreachable (not exported, not called)
        let nop2 = builder.nop();
        let func3 = Function::new(
            "func3".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(nop2),
        );

        let mut module = Module::new(&bump);
        module.add_function(func0);
        module.add_function(func1);
        module.add_function(func2_def);
        module.add_function(func3);

        // Export func0
        module.export_function(0, "main".to_string());

        let call_graph = CallGraph::build(&module);
        let analysis = GlobalAnalysis::analyze(&module, &call_graph);

        assert!(analysis.reachable_functions.contains(&0));
        assert!(analysis.reachable_functions.contains(&1));
        assert!(analysis.reachable_functions.contains(&2));
        assert!(!analysis.reachable_functions.contains(&3));
    }
}
