use crate::analysis::stats::ModuleStats;
use crate::ir::expression::{ExprRef, ExpressionKind};
use crate::ir::module::Module;
use crate::ir::visitor::Visitor;
use crate::passes::Pass;
use std::collections::HashMap;

/// Reorders types in the type section based on usage frequency.
pub struct ReorderTypes;

impl Pass for ReorderTypes {
    fn name(&self) -> &str {
        "ReorderTypes"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        if module.types.is_empty() {
            return;
        }

        let stats = ModuleStats::collect(module);
        let type_counts = stats.type_counts;

        let mut sorted_indices: Vec<usize> = (0..module.types.len()).collect();
        sorted_indices.sort_by(|&a, &b| {
            let count_a = type_counts.get(&(a as u32)).copied().unwrap_or(0);
            let count_b = type_counts.get(&(b as u32)).copied().unwrap_or(0);
            count_b.cmp(&count_a)
        });

        let mut remap = HashMap::new();
        let mut new_types = Vec::with_capacity(module.types.len());

        for (new_idx, &old_idx) in sorted_indices.iter().enumerate() {
            remap.insert(old_idx as u32, new_idx as u32);
            new_types.push(module.types[old_idx].clone());
        }

        module.types = new_types;

        // Update references
        for import in &mut module.imports {
            match &mut import.kind {
                crate::ir::module::ImportKind::Function(params, results) => {
                    *params = self.remap_type(*params, &remap);
                    *results = self.remap_type(*results, &remap);
                }
                crate::ir::module::ImportKind::Global(ty, _) => {
                    *ty = self.remap_type(*ty, &remap);
                }
                crate::ir::module::ImportKind::Table(ty, _, _) => {
                    *ty = self.remap_type(*ty, &remap);
                }
                _ => {}
            }
        }

        for func in &mut module.functions {
            if let Some(idx) = &mut func.type_idx {
                if let Some(&new_id) = remap.get(idx) {
                    *idx = new_id;
                }
            }
            func.params = self.remap_type(func.params, &remap);
            func.results = self.remap_type(func.results, &remap);
            for var in &mut func.vars {
                *var = self.remap_type(*var, &remap);
            }

            if let Some(mut body) = func.body {
                let mut updater = TypeUpdater {
                    remap: &remap,
                    pass: self,
                };
                updater.visit(&mut body);
            }
        }

        for global in &mut module.globals {
            global.type_ = self.remap_type(global.type_, &remap);
            if let Some(mut init) = Some(global.init) {
                let mut updater = TypeUpdater {
                    remap: &remap,
                    pass: self,
                };
                updater.visit(&mut init);
            }
        }
    }
}

impl ReorderTypes {
    fn remap_type(&self, ty: crate::ir::Type, remap: &HashMap<u32, u32>) -> crate::ir::Type {
        if let Some(id) = ty.signature_id() {
            if let Some(&new_id) = remap.get(&id) {
                return crate::ir::Type::from_signature_id(new_id);
            }
        }
        ty
    }
}

struct TypeUpdater<'a> {
    remap: &'a HashMap<u32, u32>,
    pass: &'a ReorderTypes,
}

impl<'a, 'b> Visitor<'b> for TypeUpdater<'a> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'b>) {
        expr.type_ = self.pass.remap_type(expr.type_, self.remap);
        match &mut expr.kind {
            ExpressionKind::CallIndirect { type_, .. }
            | ExpressionKind::StructNew { type_, .. }
            | ExpressionKind::StructGet { type_, .. }
            | ExpressionKind::StructSet { type_, .. }
            | ExpressionKind::ArrayNew { type_, .. }
            | ExpressionKind::ArrayGet { type_, .. }
            | ExpressionKind::ArraySet { type_, .. }
            | ExpressionKind::RefNull { type_ } => {
                *type_ = self.pass.remap_type(*type_, self.remap);
            }
            _ => {}
        }
        self.visit_children(expr);
    }
}
