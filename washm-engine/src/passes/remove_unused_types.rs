use crate::analysis::stats::ModuleStats;
use crate::ir::expression::{ExprRef, ExpressionKind};
use crate::ir::module::Module;
use crate::ir::visitor::Visitor;
use crate::passes::Pass;
use std::collections::HashMap;

/// Removes types that are never used in the module.
pub struct RemoveUnusedTypes;

impl Pass for RemoveUnusedTypes {
    fn name(&self) -> &str {
        "RemoveUnusedTypes"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        if module.types.is_empty() {
            return;
        }

        let stats = ModuleStats::collect(module);

        let mut remap = HashMap::new();
        let mut new_types = Vec::new();

        for (i, ty_def) in module.types.iter().enumerate() {
            let idx = i as u32;
            if stats.type_counts.get(&idx).copied().unwrap_or(0) > 0 {
                let new_idx = new_types.len() as u32;
                remap.insert(idx, new_idx);
                new_types.push(ty_def.clone());
            }
        }

        if new_types.len() == module.types.len() {
            return;
        }

        module.types = new_types;

        // Update references (same logic as ReorderTypes)
        for (_, func) in &mut module.functions {
            if let Some(idx) = &mut func.type_idx {
                if let Some(&new_id) = remap.get(idx) {
                    *idx = new_id;
                }
            }
            func.params = self.remap_type(func.params, &remap);
            func.results = self.remap_type(func.results, &remap);
            for var in func.vars_mut() {
                *var = self.remap_type(*var, &remap);
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
                updater_visit_init(init, &remap, self);
            }
        }
    }
}

fn updater_visit_init<'b>(
    expr: &mut ExprRef<'b>,
    remap: &HashMap<u32, u32>,
    pass: &RemoveUnusedTypes,
) {
    let mut updater = TypeUpdater { remap, pass };
    updater.visit(expr);
}

impl RemoveUnusedTypes {
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
    pass: &'a RemoveUnusedTypes,
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
