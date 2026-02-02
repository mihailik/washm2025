use crate::analysis::stats::ModuleStats;
use crate::collections::IdArena;
use crate::ir::expression::{ExprRef, ExpressionKind};
use crate::ir::module::{ExportKind, FuncId, FuncMarker, Module};
use crate::ir::visitor::Visitor;
use crate::passes::Pass;
use std::collections::HashMap;

/// Reorders defined functions based on access frequency.
pub struct ReorderFunctions {
    pub by_name: bool,
}

impl Pass for ReorderFunctions {
    fn name(&self) -> &str {
        if self.by_name {
            "ReorderFunctionsByName"
        } else {
            "ReorderFunctions"
        }
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        if module.functions.is_empty() {
            return;
        }

        let stats = ModuleStats::collect(module);

        // 1. Collect and sort the IDs
        let mut ids: Vec<FuncId> = module.functions.iter().map(|(id, _)| id).collect();

        if self.by_name {
            ids.sort_by(|&a, &b| module.functions[a].name.cmp(&module.functions[b].name));
        } else {
            ids.sort_by(|&a, &b| {
                let name_a = &module.functions[a].name;
                let name_b = &module.functions[b].name;
                let count_a = stats.function_counts.get(name_a).copied().unwrap_or(0);
                let count_b = stats.function_counts.get(name_b).copied().unwrap_or(0);
                // Sort descending (more frequent first)
                count_b.cmp(&count_a)
            });
        }

        // 2. Perform the reordering
        let mut old_to_new = HashMap::new();
        let mut new_functions = IdArena::new();

        // Move functions out to avoid multiple borrows
        let mut functions_map = HashMap::new();
        let old_ids: Vec<FuncId> = module.functions.iter().map(|(id, _)| id).collect();
        for id in old_ids {
            if let Some(f) = module.functions.remove(id.index() as usize) {
                functions_map.insert(id, f);
            }
        }

        for old_id in ids {
            if let Some(f) = functions_map.remove(&old_id) {
                let new_id = new_functions.alloc(f);
                old_to_new.insert(old_id, new_id);
            }
        }
        module.functions = new_functions;

        // 3. Update all references to function IDs
        // Exports
        for export in &mut module.exports {
            if let ExportKind::Function(id) = &mut export.kind {
                if let Some(&new_id) = old_to_new.get(id) {
                    *id = new_id;
                }
            }
        }

        // Start
        if let Some(start_id) = &mut module.start {
            if let Some(&new_id) = old_to_new.get(start_id) {
                *start_id = new_id;
            }
        }

        // Elements (Tables)
        for elem in &mut module.elements {
            for id in &mut elem.func_indices {
                if let Some(&new_id) = old_to_new.get(id) {
                    *id = new_id;
                }
            }
        }

        // Body updates
        for (_, func) in &mut module.functions {
            if let Some(body) = func.body_mut() {
                let mut updater = FunctionIdUpdater { remap: &old_to_new };
                updater.visit(body);
            }
        }
    }
}

struct FunctionIdUpdater<'a> {
    remap: &'a HashMap<FuncId, FuncId>,
}

impl<'a, 'b> Visitor<'b> for FunctionIdUpdater<'a> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'b>) {
        match &mut expr.kind {
            ExpressionKind::Call { target, .. } => {
                if let Some(&new_id) = self.remap.get(target) {
                    *target = new_id;
                }
            }
            ExpressionKind::RefFunc { func } => {
                if let Some(&new_id) = self.remap.get(func) {
                    *func = new_id;
                }
            }
            _ => {}
        }
        self.visit_children(expr);
    }
}
