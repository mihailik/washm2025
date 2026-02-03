use crate::analysis::call_graph::CallGraph;
use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::module::Module;
use crate::ir::visitor::ReadOnlyVisitor;
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
        let mut visitor = UsageVisitor {
            module,
            analysis: self,
        };

        for (_, func) in &module.functions {
            if let Some(body) = func.body() {
                visitor.visit(body, &module.expressions);
            }
        }

        // Visit global initializers
        for (_, global) in module.globals.iter() {
            if let crate::ir::module::GlobalKind::Local { init } = &global.kind {
                visitor.visit(*init, &module.expressions);
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
                    || (!self.written_globals.contains(&gid)
                        && !self.non_init_written.contains(&gid));

                if is_effectively_constant {
                    if let crate::ir::module::GlobalKind::Local { init } = &global.kind {
                        if let Some(val) = self.eval_constant_expression(module, *init) {
                            self.constant_globals.insert(gid);
                            self.global_values.insert(gid, val);
                            changed = true;
                        } else if !global.mutable {
                            // Even if we don't know the value, it's still constant
                            self.constant_globals.insert(gid);
                            changed = true;
                        }
                    } else if global.is_import() && !global.mutable {
                        self.constant_globals.insert(gid);
                        changed = true;
                    }
                }
            }
        }
    }

    fn eval_constant_expression(&self, module: &Module, expr_id: ExpressionId) -> Option<Literal> {
        let expr = &module.expressions[expr_id];
        match &expr.kind {
            ExpressionKind::Const(lit) => Some(*lit),
            ExpressionKind::GlobalGet { id } => self.global_values.get(id).cloned(),
            _ => None,
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
        for (_, segment) in module.elements.iter() {
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

                if let Some(callees) = call_graph.calls.get(&func_id) {
                    for &callee_id in callees {
                        if !visited.contains(&callee_id) {
                            worklist.push(callee_id);
                        }
                    }
                }
            }
        }
    }
}

struct UsageVisitor<'a> {
    module: &'a Module,
    analysis: &'a mut GlobalAnalysis,
}

impl ReadOnlyVisitor for UsageVisitor<'_> {
    fn visit_expression(
        &mut self,
        expr_id: ExpressionId,
        arena: &IdArena<ExpressionMarker, Expression>,
    ) {
        let expr = &arena[expr_id];
        match &expr.kind {
            ExpressionKind::GlobalGet { id } => {
                let id = *id;
                self.analysis.read_globals.insert(id);
                *self.analysis.read_counts.entry(id).or_insert(0) += 1;
            }
            ExpressionKind::GlobalSet { id, value } => {
                let id = *id;
                self.analysis.written_globals.insert(id);
                *self.analysis.write_counts.entry(id).or_insert(0) += 1;

                // Check if it's written with a constant value
                if let ExpressionKind::Const(lit) = &arena[*value].kind {
                    let lit = *lit;
                    if let Some(global) = self.module.globals.get(id.cast()) {
                        if let crate::ir::module::GlobalKind::Local { init } = &global.kind {
                            if let ExpressionKind::Const(init_lit) = &arena[*init].kind {
                                if lit != *init_lit {
                                    self.analysis.non_init_written.insert(id);
                                }
                            } else {
                                // Non-const initializer, any write is non-init (effectively)
                                self.analysis.non_init_written.insert(id);
                            }
                        }
                    }
                } else {
                    // Non-const value being set
                    self.analysis.non_init_written.insert(id);
                }
            }
            _ => {}
        }
        self.visit_children(expr_id, arena);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::IrBuilder;
    use crate::ir::module::Function;
    use crate::ir::Type;

    #[test]
    fn test_reachability() {
        let mut module = Module::new();
        let (arena, _, _, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        let mut builder = IrBuilder::new(arena);

        // Dummy IDs for builder.call (will be replaced after adding to module)
        // We'll build the expressions first

        // Func 0: Exported, calls Func 1
        let f1_id: FuncId = 1.into();
        let f2_id: FuncId = 2.into();

        let call1 = builder.call(f1_id, vec![], Type::NONE, false);
        let f0 = Function::new("f0".to_string(), Type::NONE, Type::NONE, vec![], call1);

        // Func 1: Internal, calls Func 2
        let call2 = builder.call(f2_id, vec![], Type::NONE, false);
        let f1 = Function::new("f1".to_string(), Type::NONE, Type::NONE, vec![], call2);

        // Func 2: Internal, leaf
        let nop = builder.nop();
        let f2 = Function::new("f2".to_string(), Type::NONE, Type::NONE, vec![], nop);

        // Func 3: Unreachable
        let nop2 = builder.nop();
        let f3 = Function::new("f3".to_string(), Type::NONE, Type::NONE, vec![], nop2);

        let id0 = module.add_function(f0);
        let _id1 = module.add_function(f1);
        let _id2 = module.add_function(f2);
        let id3 = module.add_function(f3);

        module.add_export(
            "f0".to_string(),
            crate::ir::module::ExportKind::Function(id0),
        );

        let call_graph = CallGraph::build(&module);
        let analysis = GlobalAnalysis::analyze(&module, &call_graph);

        assert!(analysis.reachable_functions.contains(&id0));
        assert!(analysis.reachable_functions.contains(&1.into()));
        assert!(analysis.reachable_functions.contains(&2.into()));
        assert!(!analysis.reachable_functions.contains(&id3));
    }
}
