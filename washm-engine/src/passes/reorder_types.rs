use crate::analysis::stats::ModuleStats;
use crate::collections::{Id, IdArena};
use crate::ir::expression::{ExprRef, ExpressionKind};
use crate::ir::module::{Module, TypeId};
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
        let mut new_arena = IdArena::new();

        for &old_idx in &sorted_indices {
            let old_id = Id::from_u32(old_idx as u32);
            if let Some(ty) = module.types.get(old_id) {
                let new_id = new_arena.alloc(ty.clone());
                remap.insert(old_id, new_id);
            }
        }

        module.types = new_arena;

        // Update references
        for (_, func) in &mut module.functions {
            if let Some(idx) = &mut func.type_idx {
                if let Some(&new_id) = remap.get(idx) {
                    *idx = new_id;
                }
            }
            func.params = self.remap_type(func.params, &remap);
            func.results = self.remap_type(func.results, &remap);
            for (_, var) in func.locals.iter_mut() {
                var.type_ = self.remap_type(var.type_, &remap);
            }

            if let Some(body) = func.body_mut() {
                let mut updater = TypeUpdater {
                    remap: &remap,
                    pass: self,
                };
                updater.visit(body);
            }
        }

        for (_, global) in &mut module.globals {
            global.type_ = self.remap_type(global.type_, &remap);
            if let crate::ir::module::GlobalKind::Local { ref mut init } = global.kind {
                let mut updater = TypeUpdater {
                    remap: &remap,
                    pass: self,
                };
                updater.visit(init);
            }
        }
    }
}

impl ReorderTypes {
    fn remap_type(&self, ty: crate::ir::Type, remap: &HashMap<TypeId, TypeId>) -> crate::ir::Type {
        if let Some(id) = ty.signature_id() {
            let old_type_id = Id::from_u32(id);
            if let Some(&new_id) = remap.get(&old_type_id) {
                return crate::ir::Type::from_signature_id(new_id.index);
            }
        }
        ty
    }
}

struct TypeUpdater<'a> {
    remap: &'a HashMap<TypeId, TypeId>,
    pass: &'a ReorderTypes,
}

impl<'a, 'b> Visitor<'b> for TypeUpdater<'a> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'b>) {
        expr.type_ = self.pass.remap_type(expr.type_, self.remap);
        match &mut expr.kind {
            ExpressionKind::CallIndirect { type_id, .. } => {
                let ty = crate::ir::Type::from_signature_id(type_id.index());
                let new_ty = self.pass.remap_type(ty, self.remap);
                if let Some(new_id) = new_ty.signature_id() {
                    *type_id = crate::ir::TypeId::from_u32(new_id);
                }
            }
            ExpressionKind::StructNew { type_, .. }
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
