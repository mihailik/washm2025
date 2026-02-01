use crate::analysis::stats::ModuleStats;
use crate::ir::module::{ExportKind, ImportKind, Module};
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

        // 1. Collect and sort the functions
        let mut sorted_indices: Vec<usize> = (0..module.functions.len()).collect();

        if self.by_name {
            sorted_indices
                .sort_by(|&a, &b| module.functions[a].name.cmp(&module.functions[b].name));
        } else {
            sorted_indices.sort_by(|&a, &b| {
                let count_a = stats
                    .function_counts
                    .get(&module.functions[a].name)
                    .copied()
                    .unwrap_or(0);
                let count_b = stats
                    .function_counts
                    .get(&module.functions[b].name)
                    .copied()
                    .unwrap_or(0);
                // Sort descending (more frequent first)
                count_b.cmp(&count_a)
            });
        }

        // 2. Perform the reordering
        let mut old_to_new = HashMap::new();

        let import_count = module
            .imports
            .iter()
            .filter(|i| matches!(i.kind, ImportKind::Function(_, _)))
            .count() as u32;

        for (new_idx, &old_idx) in sorted_indices.iter().enumerate() {
            old_to_new.insert(import_count + old_idx as u32, import_count + new_idx as u32);
        }

        // Reorder
        let old_functions = std::mem::take(&mut module.functions);

        let mut options: Vec<Option<crate::ir::module::Function<'a>>> =
            old_functions.into_iter().map(Some).collect();
        for &idx in &sorted_indices {
            module.functions.push(options[idx].take().unwrap());
        }

        // 3. Update all references to function indices
        // Exports
        for export in &mut module.exports {
            if let ExportKind::Function(idx) = &mut export.kind {
                if let Some(&new_idx) = old_to_new.get(idx) {
                    *idx = new_idx;
                }
            }
        }

        // Start
        if let Some(start_idx) = &mut module.start {
            if let Some(&new_idx) = old_to_new.get(start_idx) {
                *start_idx = new_idx;
            }
        }

        // Elements (Tables)
        for elem in &mut module.elements {
            for idx in &mut elem.func_indices {
                if let Some(&new_idx) = old_to_new.get(idx) {
                    *idx = new_idx;
                }
            }
        }
    }
}

// Helper trait to allow "taking" from a Vec of Functions
#[allow(dead_code)]
trait TakeFunction<'a> {
    fn take(&mut self) -> crate::ir::module::Function<'a>;
}

impl<'a> TakeFunction<'a> for crate::ir::module::Function<'a> {
    fn take(&mut self) -> crate::ir::module::Function<'a> {
        std::mem::replace(
            self,
            crate::ir::module::Function {
                name: String::new(),
                type_idx: None,
                params: crate::ir::Type::NONE,
                results: crate::ir::Type::NONE,
                vars: Vec::new(),
                body: None,
                local_names: Vec::new(),
            },
        )
    }
}
