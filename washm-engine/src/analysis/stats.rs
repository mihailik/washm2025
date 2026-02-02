use crate::ir::expression::{ExprRef, ExpressionKind};
use crate::ir::module::{GlobalKind, Module, TableKind};
use crate::ir::visitor::ReadOnlyVisitor;
use std::collections::HashMap;

/// Collects statistics about module elements to aid in reorganization.
#[derive(Debug, Default)]
pub struct ModuleStats {
    pub function_counts: HashMap<String, usize>,
    pub global_counts: HashMap<crate::ir::GlobalId, usize>,
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

        // Count references in functions (including imports)
        for (_, func) in &module.functions {
            if let Some(import) = &func.import {
                stats.increment_type(func.params);
                stats.increment_type(func.results);
            } else {
                stats.increment_type(func.params);
                stats.increment_type(func.results);
                for &ty in &func.vars {
                    stats.increment_type(ty);
                }

                if let Some(body_expr) = &func.body {
                    let mut visitor = StatsVisitor {
                        stats: &mut stats,
                        module,
                        current_function: Some(func.name.clone()),
                    };
                    visitor.visit_expression(*body_expr);
                }
            }
            if let Some(idx) = func.type_idx {
                *stats.type_counts.entry(idx).or_insert(0) += 1;
            }
        }

        // Count references in tables
        for (_, table) in &module.tables {
            stats.increment_type(table.limits.element_type);
        }

        // Count references in memories
        // (Memory doesn't have a type that we count usually, just limits)

        // Count references in globals
        for (_, global) in &module.globals {
            stats.increment_type(global.type_);
            if let GlobalKind::Local { init } = &global.kind {
                let mut visitor = StatsVisitor {
                    stats: &mut stats,
                    module,
                    current_function: None,
                };
                visitor.visit_expression(*init);
            }
        }

        // Count references in exports
        for export in &module.exports {
            match export.kind {
                crate::ir::module::ExportKind::Function(id) => {
                    if let Some(func) = module.functions.get(id) {
                        stats.increment_func(&func.name);
                    }
                }
                crate::ir::module::ExportKind::Global(id) => {
                    stats.increment_global(id);
                }
                _ => {}
            }
        }

        // Count references in start function
        if let Some(start_id) = module.start {
            if let Some(func) = module.functions.get(start_id) {
                stats.increment_func(&func.name);
            }
        }

        // Count references in table elements
        for elem in &module.elements {
            for &id in &elem.func_indices {
                if let Some(func) = module.functions.get(id) {
                    stats.increment_func(&func.name);
                }
            }
        }

        stats
    }

    fn increment_func(&mut self, name: &str) {
        *self.function_counts.entry(name.to_string()).or_insert(0) += 1;
    }

    fn increment_global(&mut self, id: crate::ir::GlobalId) {
        *self.global_counts.entry(id).or_insert(0) += 1;
    }

    fn increment_type(&mut self, ty: crate::ir::Type) {
        if let Some(id) = ty.signature_id() {
            *self.type_counts.entry(id).or_insert(0) += 1;
        }
    }
}

struct StatsVisitor<'a, 'b> {
    stats: &'a mut ModuleStats,
    module: &'a Module<'b>,
    current_function: Option<String>,
}

impl<'a, 'b, 'c> ReadOnlyVisitor<'c> for StatsVisitor<'a, 'b> {
    fn visit_expression(&mut self, expr: ExprRef<'c>) {
        self.stats.increment_type(expr.type_);
        match &expr.kind {
            ExpressionKind::Call { target, .. } | ExpressionKind::RefFunc { func: target } => {
                if let Some(func) = self.module.functions.get(*target) {
                    self.stats.increment_func(&func.name);
                }
            }
            ExpressionKind::GlobalGet { id } | ExpressionKind::GlobalSet { id, .. } => {
                self.stats.increment_global(*id);
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
