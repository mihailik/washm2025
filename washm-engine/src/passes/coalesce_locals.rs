use crate::analysis::dataflow::cfg::CFGBuilder;
use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::module::{Function, Module};
use crate::ir::visitor::Visitor;
use crate::ir::Type;
use crate::passes::Pass;
use std::collections::{HashMap, HashSet};

pub struct CoalesceLocals;

impl Pass for CoalesceLocals {
    fn name(&self) -> &str {
        "coalesce-locals"
    }

    fn run(&mut self, module: &mut Module) {
        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        let ids: Vec<_> = functions.iter().map(|(id, _)| id).collect();
        for id in ids {
            Self::run_on_func(&mut functions[id.cast()], arena);
        }
    }
}

struct LocalMapper<'a> {
    mapping: &'a HashMap<crate::ir::LocalId, crate::ir::LocalId>,
}

impl<'a> Visitor for LocalMapper<'a> {
    fn visit(
        &mut self,
        expr_id: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        let expr = &mut arena[*expr_id];
        match &mut expr.kind {
            ExpressionKind::LocalGet { id } => {
                if let Some(&new_id) = self.mapping.get(id) {
                    *id = new_id;
                }
            }
            ExpressionKind::LocalSet { id, .. } | ExpressionKind::LocalTee { id, .. } => {
                if let Some(&new_id) = self.mapping.get(id) {
                    *id = new_id;
                }
            }
            _ => {}
        }
        self.visit_children(expr_id, arena);
    }
}

struct CopyCoalescingVisitor;

impl Visitor for CopyCoalescingVisitor {
    fn visit(
        &mut self,
        expr_id: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        self.visit_children(expr_id, arena);

        let expr = &arena[*expr_id];
        if let ExpressionKind::LocalSet { id: set_id, value } = &expr.kind {
            let value_id = *value;
            let value_expr = &arena[value_id];
            if let ExpressionKind::LocalGet { id: get_id } = &value_expr.kind {
                if set_id == get_id {
                    // This is a redundant copy: local.set X (local.get X)
                    let expr_mut = &mut arena[*expr_id];
                    if expr_mut.type_ != Type::NONE {
                        expr_mut.kind = ExpressionKind::Drop { value: value_id };
                    } else {
                        expr_mut.kind = ExpressionKind::Nop;
                    }
                    expr_mut.type_ = Type::NONE;
                }
            }
        }
    }
}

impl CoalesceLocals {
    pub fn run_on_func(func: &mut Function, arena: &mut IdArena<ExpressionMarker, Expression>) {
        if func.is_import() {
            return;
        }
        let num_params = func.num_params();

        let mut all_locals: Vec<(crate::ir::LocalId, Type)> = Vec::new();
        for &id in func.param_ids() {
            all_locals.push((id, func.locals().get(id.cast()).unwrap().type_));
        }
        let param_set: std::collections::HashSet<crate::ir::LocalId> =
            func.param_ids().iter().cloned().collect();
        for (id, local) in func.locals().iter() {
            if !param_set.contains(&id.cast()) {
                all_locals.push((id.cast(), local.type_));
            }
        }

        let types: Vec<Type> = all_locals.iter().map(|(_, t)| *t).collect();

        let mut mapping: HashMap<crate::ir::LocalId, crate::ir::LocalId> = HashMap::new();

        if let Some(body_id) = func.body() {
            let builder = CFGBuilder::new(arena);
            let mut cfg = builder.build(body_id);
            cfg.calculate_liveness();

            let interference = cfg.calculate_interference();

            // Internal indices for coloring
            let num_locals = all_locals.len();
            let mut internal_mapping: Vec<usize> = (0..num_locals).collect();
            let mut new_vars: Vec<Vec<usize>> = Vec::new();

            for i in num_params..num_locals {
                let mut found = false;
                for (j, assigned) in new_vars.iter_mut().enumerate() {
                    if assigned.is_empty() {
                        continue;
                    }
                    let type_j = types[assigned[0]];
                    if types[i] != type_j {
                        continue;
                    }

                    let mut interferes = false;
                    for &other_idx in assigned.iter() {
                        if interference.interferes(all_locals[i].0, all_locals[other_idx].0) {
                            interferes = true;
                            break;
                        }
                    }

                    if !interferes {
                        internal_mapping[i] = num_params + j;
                        assigned.push(i);
                        found = true;
                        break;
                    }
                }

                if !found {
                    internal_mapping[i] = num_params + new_vars.len();
                    new_vars.push(vec![i]);
                }
            }

            // Map internal indices back to LocalIds
            let mut color_to_id = HashMap::new();
            for (i, local) in all_locals.iter().enumerate().take(num_params) {
                color_to_id.insert(i, local.0);
            }
            for i in num_params..num_locals {
                let color = internal_mapping[i];
                color_to_id.entry(color).or_insert(all_locals[i].0);
            }

            for i in num_params..num_locals {
                let color = internal_mapping[i];
                let target_id = color_to_id[&color];
                if all_locals[i].0 != target_id {
                    mapping.insert(all_locals[i].0, target_id);
                }
            }
        }

        if !mapping.is_empty() {
            if let Some(body_id) = func.body_mut() {
                // Remap instructions
                let mut mapper = LocalMapper { mapping: &mapping };
                mapper.visit(body_id, arena);

                // Remove dead stores after coalescing and remapping
                Self::eliminate_dead_stores(body_id, arena);
            }

            // Remove merged locals
            for &old_id in mapping.keys() {
                func.locals_mut().remove_id(old_id);
            }

            if let Some(body_id) = func.body_mut() {
                // Perform copy coalescing after remapping and dead store elimination
                let mut copy_coalescer = CopyCoalescingVisitor;
                copy_coalescer.visit(body_id, arena);
            }
        }
    }

    fn eliminate_dead_stores(
        root: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        let mut collector = UsedLocalCollector {
            used: HashSet::new(),
        };
        collector.visit(root, arena);

        let mut eliminator = DeadStoreEliminator {
            used: &collector.used,
        };
        eliminator.visit(root, arena);
    }
}

struct UsedLocalCollector {
    used: HashSet<crate::ir::LocalId>,
}
impl Visitor for UsedLocalCollector {
    fn visit(
        &mut self,
        expr_id: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        if let ExpressionKind::LocalGet { id } = &arena[*expr_id].kind {
            self.used.insert(*id);
        }
        self.visit_children(expr_id, arena);
    }
}

struct DeadStoreEliminator<'a> {
    used: &'a HashSet<crate::ir::LocalId>,
}
impl<'a> Visitor for DeadStoreEliminator<'a> {
    fn visit(
        &mut self,
        expr_id: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        let expr = &mut arena[*expr_id];
        match &mut expr.kind {
            ExpressionKind::LocalSet { id, value } | ExpressionKind::LocalTee { id, value } => {
                let value_id = *value;
                if !self.used.contains(id) {
                    expr.kind = ExpressionKind::Drop { value: value_id };
                }
            }
            _ => {}
        }
        self.visit_children(expr_id, arena);
    }
}
