use crate::analysis::call_graph::CallGraph;
use crate::ir::expression::{ExprRef, ExpressionKind};
use crate::ir::module::Module;
use crate::ir::{FuncId, GlobalId, Literal};
use std::collections::{HashMap, HashSet};

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
    pub reachable_functions: HashSet<FuncId>,
}

impl Default for GlobalAnalysis {
    fn default() -> Self {
        Self::new()
    }
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
        for (_, func) in &module.functions {
            if let Some(body) = func.body() {
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

                let is_effectively_constant = !global.mutable
                    || !self.written_globals.contains(&gid)
                    || !self.non_init_written.contains(&gid);

                if is_effectively_constant {
                    self.constant_globals.insert(gid);

                    // Try to resolve the value
                    if let crate::ir::module::GlobalKind::Local { init } = &global.kind {
                        if let ExpressionKind::Const(value) = &init.kind {
                            if let std::collections::hash_map::Entry::Vacant(e) =
                                self.global_values.entry(gid)
                            {
                                e.insert(*value);
                                changed = true;
                            }
                        } else if let ExpressionKind::GlobalGet { id: other_id } = &init.kind {
                            if let Some(value) = self.global_values.get(other_id) {
                                let val = *value;
                                if !(self.global_values.get(&gid) == Some(&val)) {
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
            ExpressionKind::Return { value: Some(v) } => {
                self.visit_usage(module, *v);
            }
            ExpressionKind::Return { value: None } => {}
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
        // Roots: exports, start function, table elements?
        let mut roots = HashSet::new();

        // 1. Exports
        for export in &module.exports {
            if let crate::ir::module::ExportKind::Function(idx) = export.kind {
                roots.insert(idx);
            }
        }

        // 2. Start
        if let Some(start) = module.start {
            roots.insert(start);
        }

        // 3. Elements
        for segment in module.elements.iter() {
            for &func_id in &segment.func_indices {
                roots.insert(func_id);
            }
        }

        // Compute reachability
        let mut visited = HashSet::new();
        let mut worklist: Vec<FuncId> = roots.into_iter().collect();

        while let Some(func_id) = worklist.pop() {
            if !visited.contains(&func_id) {
                visited.insert(func_id);
                self.reachable_functions.insert(func_id);

                let func = &module.functions[func_id];
                if let Some(callees) = call_graph.calls.get(&func.name) {
                    for callee_name in callees {
                        if let Some((callee_id, _)) = module.get_function_by_name(callee_name) {
                            if !visited.contains(&callee_id) {
                                worklist.push(callee_id);
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

        // Dummy IDs for builder.call (will update after adding to module)
        let f1_placeholder: FuncId = 1.into();
        let f2_placeholder: FuncId = 2.into();

        // Func 0: Exported, calls Func 1
        let call1 = builder.call(
            f1_placeholder,
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
            f2_placeholder,
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
        let f0_id = module.add_function(func0);
        let f1_id = module.add_function(func1);
        let f2_id = module.add_function(func2_def);
        let f3_id = module.add_function(func3);

        // Patch placeholder IDs
        if let ExpressionKind::Call { target, .. } = &mut module.functions[f0_id].body.unwrap().kind
        {
            *target = f1_id;
        }
        if let ExpressionKind::Call { target, .. } = &mut module.functions[f1_id].body.unwrap().kind
        {
            *target = f2_id;
        }

        // Export func0
        module.export_function(f0_id, "main".to_string());

        let call_graph = CallGraph::build(&module);
        let analysis = GlobalAnalysis::analyze(&module, &call_graph);

        assert!(analysis.reachable_functions.contains(&f0_id));
        assert!(analysis.reachable_functions.contains(&f1_id));
        assert!(analysis.reachable_functions.contains(&f2_id));
        assert!(!analysis.reachable_functions.contains(&f3_id));
    }
}
