use crate::analysis::stats::ModuleStats;
use crate::ir::expression::{ExprRef, ExpressionKind};
use crate::ir::module::{ExportKind, ImportKind, Module};
use crate::passes::Pass;
use crate::ir::visitor::Visitor;
use std::collections::HashMap;

/// Reorders globals based on access frequency.
pub struct ReorderGlobals;

impl Pass for ReorderGlobals {
    fn name(&self) -> &str {
        "ReorderGlobals"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        if module.globals.is_empty() {
            return;
        }

        let stats = ModuleStats::collect(module);

        // 1. Determine reordering for defined globals
        let import_global_count = module
            .imports
            .iter()
            .filter(|i| matches!(i.kind, ImportKind::Global(_, _)))
            .count() as u32;

        // We only reorder defined globals (module.globals)
        let mut sorted_indices: Vec<usize> = (0..module.globals.len()).collect();
        sorted_indices.sort_by(|&a, &b| {
            let count_a = stats
                .global_counts
                .get(&(import_global_count + a as u32))
                .copied()
                .unwrap_or(0);
            let count_b = stats
                .global_counts
                .get(&(import_global_count + b as u32))
                .copied()
                .unwrap_or(0);
            count_b.cmp(&count_a)
        });

        let mut remap = HashMap::new();
        let mut new_globals = Vec::with_capacity(module.globals.len());

        for (new_idx, &old_idx) in sorted_indices.iter().enumerate() {
            remap.insert(
                import_global_count + old_idx as u32,
                import_global_count + new_idx as u32,
            );
        }

        // Reorder
        let mut old_globals = std::mem::take(&mut module.globals);
        for &idx in sorted_indices.iter() {
            new_globals.push(std::mem::replace(
                &mut old_globals[idx],
                crate::ir::module::Global {
                    name: String::new(),
                    type_: crate::ir::Type::NONE,
                    mutable: false,
                    init: ExprRef::new(module.allocator.alloc(crate::ir::expression::Expression {
                        kind: ExpressionKind::Nop,
                        type_: crate::ir::Type::NONE,
                    })),
                },
            ));
        }
        module.globals = new_globals;

        // 2. Update references
        // Exports
        for export in &mut module.exports {
            if export.kind == ExportKind::Global {
                if let Some(&new_idx) = remap.get(&export.index) {
                    export.index = new_idx;
                }
            }
        }

        // Expressions
        let mut updater = GlobalUpdater { remap: &remap };
        for func in &mut module.functions {
            if let Some(mut body) = func.body {
                updater.visit(&mut body);
            }
        }
        for global in &mut module.globals {
            updater.visit(&mut global.init);
        }
    }
}

struct GlobalUpdater<'a> {
    remap: &'a HashMap<u32, u32>,
}

impl<'a, 'b> Visitor<'b> for GlobalUpdater<'a> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'b>) {
        self.visit_children(expr);
        match &mut expr.kind {
            ExpressionKind::GlobalGet { index } | ExpressionKind::GlobalSet { index, .. } => {
                if let Some(&new_idx) = self.remap.get(index) {
                    *index = new_idx;
                }
            }
            _ => {}
        }
    }
}
