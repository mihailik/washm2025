use crate::collections::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::module::{FunctionKind, GlobalKind, Module};
use crate::ir::visitor::ReadOnlyVisitor;
use std::collections::HashMap;

/// Collects statistics about module elements to aid in reorganization.
#[derive(Debug, Default)]
pub struct ModuleStats {
    pub function_counts: HashMap<String, usize>,
    pub global_counts: HashMap<crate::ir::GlobalId, usize>,
    pub type_counts: HashMap<u32, usize>,
    pub local_counts: HashMap<String, HashMap<crate::ir::LocalId, usize>>,
}

impl ModuleStats {
    pub fn collect(module: &Module) -> Self {
        let mut stats = Self::default();

        // Count types in module signatures
        for (_, ty) in &module.types {
            stats.increment_type(ty.params);
            stats.increment_type(ty.results);
        }

        // Count references in functions (including imports)
        for (_, func) in &module.functions {
            stats.increment_type(func.params);
            stats.increment_type(func.results);

            match &func.kind {
                FunctionKind::Import(_) => {}
                FunctionKind::Local { locals, body, .. } => {
                    for local in locals.values() {
                        stats.increment_type(local.type_);
                    }

                    let mut visitor = StatsVisitor {
                        stats: &mut stats,
                        module,
                        current_function: Some(func.name.clone()),
                    };
                    visitor.visit(*body, &module.expressions);
                }
            }

            if let Some(idx) = func.type_idx {
                *stats.type_counts.entry(idx.index()).or_insert(0) += 1;
            }
        }

        // Count references in tables
        for (_, table) in &module.tables {
            stats.increment_type(table.limits.element_type);
        }

        // Count references in globals
        for (_, global) in &module.globals {
            stats.increment_type(global.type_);
            if let GlobalKind::Local { init } = &global.kind {
                let mut visitor = StatsVisitor {
                    stats: &mut stats,
                    module,
                    current_function: None,
                };
                visitor.visit(*init, &module.expressions);
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
        for (_, elem) in &module.elements {
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

struct StatsVisitor<'a> {
    stats: &'a mut ModuleStats,
    module: &'a Module,
    current_function: Option<String>,
}

impl<'a> ReadOnlyVisitor for StatsVisitor<'a> {
    fn visit_expression(
        &mut self,
        expr_id: ExpressionId,
        arena: &IdArena<ExpressionMarker, Expression>,
    ) {
        let expr = &arena[expr_id];
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
            ExpressionKind::LocalGet { id }
            | ExpressionKind::LocalSet { id, .. }
            | ExpressionKind::LocalTee { id, .. } => {
                if let Some(func_name) = &self.current_function {
                    *self
                        .stats
                        .local_counts
                        .entry(func_name.clone())
                        .or_default()
                        .entry(*id)
                        .or_insert(0) += 1;
                }
            }
            ExpressionKind::CallIndirect { type_id, .. } => {
                self.stats
                    .increment_type(crate::ir::Type::from_signature_id(type_id.index()));
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
        self.visit_children(expr_id, arena);
    }
}
