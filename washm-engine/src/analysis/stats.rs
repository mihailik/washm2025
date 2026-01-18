use crate::ir::expression::{ExprRef, ExpressionKind};
use crate::ir::module::{ImportKind, Module};
use crate::ir::visitor::ReadOnlyVisitor;
use std::collections::HashMap;

/// Collects statistics about module elements to aid in reorganization.
#[derive(Debug, Default)]
pub struct ModuleStats {
    pub function_counts: HashMap<String, usize>,
    pub global_counts: HashMap<u32, usize>,
    pub type_counts: HashMap<u32, usize>,
    pub local_counts: HashMap<String, HashMap<u32, usize>>,
}

impl ModuleStats {
    pub fn collect(module: &Module) -> Self {
        let mut stats = Self::default();

        // Count types in module signatures
        for ty in &module.types {
            stats.increment_type(ty.params);
            stats.increment_type(ty.results);
        }

        // Count references in imports
        for import in &module.imports {
            match import.kind {
                ImportKind::Function(params, results) => {
                    stats.increment_type(params);
                    stats.increment_type(results);
                }
                ImportKind::Global(ty, _) => {
                    stats.increment_type(ty);
                }
                ImportKind::Table(ty, _, _) => {
                    stats.increment_type(ty);
                }
                _ => {}
            }
        }

        // Count references in exports
        for export in &module.exports {
            match export.kind {
                crate::ir::module::ExportKind::Function => {
                    if let Some(name) = module_get_func_name(module, export.index) {
                        stats.increment_func(&name);
                    }
                }
                crate::ir::module::ExportKind::Global => {
                    stats.increment_global(export.index);
                }
                _ => {}
            }
        }

        // Count references in functions
        for func in &module.functions {
            stats.increment_type(func.params);
            stats.increment_type(func.results);
            for &ty in &func.vars {
                stats.increment_type(ty);
            }
            if let Some(idx) = func.type_idx {
                *stats.type_counts.entry(idx).or_insert(0) += 1;
            }

            if let Some(body) = func.body {
                let mut visitor = StatsVisitor {
                    stats: &mut stats,
                    current_function: Some(func.name.clone()),
                };
                visitor.visit_expression(body);
            }
        }

        // Count references in start function
        if let Some(start_idx) = module.start {
            if let Some(name) = module_get_func_name(module, start_idx) {
                stats.increment_func(&name);
            }
        }

        // Count references in table elements
        for elem in &module.elements {
            for &idx in &elem.func_indices {
                if let Some(name) = module_get_func_name(module, idx) {
                    stats.increment_func(&name);
                }
            }
        }

        // Count references in global init expressions
        for global in &module.globals {
            stats.increment_type(global.type_);
            let mut visitor = StatsVisitor {
                stats: &mut stats,
                current_function: None,
            };
            visitor.visit_expression(global.init);
        }

        stats
    }

    fn increment_func(&mut self, name: &str) {
        *self.function_counts.entry(name.to_string()).or_insert(0) += 1;
    }

    fn increment_global(&mut self, index: u32) {
        *self.global_counts.entry(index).or_insert(0) += 1;
    }

    fn increment_type(&mut self, ty: crate::ir::Type) {
        if let Some(id) = ty.signature_id() {
            *self.type_counts.entry(id).or_insert(0) += 1;
        }
    }
}

fn module_get_func_name(module: &Module, index: u32) -> Option<String> {
    let mut current_idx = 0;
    for import in &module.imports {
        if let ImportKind::Function(_, _) = import.kind {
            if current_idx == index {
                return Some(import.name.clone());
            }
            current_idx += 1;
        }
    }

    let defined_idx = index - current_idx;
    module
        .functions
        .get(defined_idx as usize)
        .map(|f| f.name.clone())
}

struct StatsVisitor<'a> {
    stats: &'a mut ModuleStats,
    current_function: Option<String>,
}

impl<'a, 'b> ReadOnlyVisitor<'b> for StatsVisitor<'a> {
    fn visit_expression(&mut self, expr: ExprRef<'b>) {
        self.stats.increment_type(expr.type_);
        match &expr.kind {
            ExpressionKind::Call { target, .. } | ExpressionKind::RefFunc { func: target } => {
                self.stats.increment_func(target);
            }
            ExpressionKind::GlobalGet { index } | ExpressionKind::GlobalSet { index, .. } => {
                self.stats.increment_global(*index);
            }
            ExpressionKind::LocalGet { index }
            | ExpressionKind::LocalSet { index, .. }
            | ExpressionKind::LocalTee { index, .. } => {
                if let Some(func_name) = &self.current_function {
                    *self
                        .stats
                        .local_counts
                        .entry(func_name.clone())
                        .or_default()
                        .entry(*index)
                        .or_insert(0) += 1;
                }
            }
            ExpressionKind::CallIndirect { type_, .. } => {
                self.stats.increment_type(*type_);
            }
            ExpressionKind::StructNew { type_, .. }
            | ExpressionKind::StructGet { type_, .. }
            | ExpressionKind::StructSet { type_, .. }
            | ExpressionKind::ArrayNew { type_, .. }
            | ExpressionKind::ArrayGet { type_, .. }
            | ExpressionKind::ArraySet { type_, .. }
            | ExpressionKind::RefNull { type_ } => {
                self.stats.increment_type(*type_);
            }
            _ => {}
        }
        self.visit_children(expr);
    }
}
