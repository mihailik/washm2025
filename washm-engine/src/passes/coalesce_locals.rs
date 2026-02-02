use crate::analysis::dataflow::cfg::CFGBuilder;
use crate::ir::expression::{ExprRef, ExpressionKind};
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

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        for (_, func) in &mut module.functions {
            Self::run_on_func(func);
        }
    }
}

struct LocalMapper<'a> {
    mapping: &'a HashMap<crate::ir::LocalId, crate::ir::LocalId>,
}

impl<'a, 'b> Visitor<'a> for LocalMapper<'b> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        match &mut expr.kind {
            ExpressionKind::LocalGet { id } => {
                if let Some(&new_id) = self.mapping.get(id) {
                    *id = new_id;
                }
            }
            ExpressionKind::LocalSet { id, value } => {
                self.visit(value);
                if let Some(&new_id) = self.mapping.get(id) {
                    *id = new_id;
                }
            }
            ExpressionKind::LocalTee { id, value } => {
                self.visit(value);
                if let Some(&new_id) = self.mapping.get(id) {
                    *id = new_id;
                }
            }
            _ => {
                self.visit_children(expr);
            }
        }
    }
}

struct CopyCoalescingVisitor;

impl<'a> Visitor<'a> for CopyCoalescingVisitor {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        // Manually visit children for recursive traversal based on ExpressionKind variants
        if let Some(expr_mut) = unsafe { expr.as_ptr().as_mut() } {
            match &mut expr_mut.kind {
                ExpressionKind::Block { list, .. } => {
                    for e in list.iter_mut() {
                        self.visit_expression(e);
                    }
                }
                ExpressionKind::If {
                    condition,
                    if_true,
                    if_false,
                } => {
                    self.visit_expression(condition);
                    self.visit_expression(if_true);
                    if let Some(f) = if_false {
                        self.visit_expression(f);
                    }
                }
                ExpressionKind::Loop { body, .. } => {
                    self.visit_expression(body);
                }
                ExpressionKind::Unary { value, .. } => {
                    self.visit_expression(value);
                }
                ExpressionKind::Binary { left, right, .. } => {
                    self.visit_expression(left);
                    self.visit_expression(right);
                }
                ExpressionKind::Call { operands, .. } => {
                    for e in operands.iter_mut() {
                        self.visit_expression(e);
                    }
                }
                ExpressionKind::LocalSet { value, .. } => {
                    self.visit_expression(value);
                }
                ExpressionKind::LocalTee { value, .. } => {
                    self.visit_expression(value);
                }
                ExpressionKind::GlobalSet { value, .. } => {
                    self.visit_expression(value);
                }
                ExpressionKind::Break {
                    condition, value, ..
                } => {
                    if let Some(c) = condition {
                        self.visit_expression(c);
                    }
                    if let Some(v) = value {
                        self.visit_expression(v);
                    }
                }
                ExpressionKind::Return { value } => {
                    if let Some(v) = value {
                        self.visit_expression(v);
                    }
                }
                ExpressionKind::Drop { value } => {
                    self.visit_expression(value);
                }
                ExpressionKind::Select {
                    condition,
                    if_true,
                    if_false,
                } => {
                    self.visit_expression(condition);
                    self.visit_expression(if_true);
                    self.visit_expression(if_false);
                }
                ExpressionKind::Load { ptr, .. } => {
                    self.visit_expression(ptr);
                }
                ExpressionKind::Store { ptr, value, .. } => {
                    self.visit_expression(ptr);
                    self.visit_expression(value);
                }
                ExpressionKind::Switch {
                    condition, value, ..
                } => {
                    self.visit_expression(condition);
                    if let Some(v) = value {
                        self.visit_expression(v);
                    }
                }
                ExpressionKind::CallIndirect {
                    target, operands, ..
                } => {
                    self.visit_expression(target);
                    for e in operands.iter_mut() {
                        self.visit_expression(e);
                    }
                }
                ExpressionKind::MemoryGrow { delta, .. } => {
                    self.visit_expression(delta);
                }
                ExpressionKind::AtomicRMW { ptr, value, .. } => {
                    self.visit_expression(ptr);
                    self.visit_expression(value);
                }
                ExpressionKind::AtomicCmpxchg {
                    ptr,
                    expected,
                    replacement,
                    ..
                } => {
                    self.visit_expression(ptr);
                    self.visit_expression(expected);
                    self.visit_expression(replacement);
                }
                ExpressionKind::AtomicWait {
                    ptr,
                    expected,
                    timeout,
                    ..
                } => {
                    self.visit_expression(ptr);
                    self.visit_expression(expected);
                    self.visit_expression(timeout);
                }
                ExpressionKind::AtomicNotify { ptr, count, .. } => {
                    self.visit_expression(ptr);
                    self.visit_expression(count);
                }
                ExpressionKind::TupleMake { operands } => {
                    for e in operands.iter_mut() {
                        self.visit_expression(e);
                    }
                }
                ExpressionKind::TupleExtract { tuple, .. } => {
                    self.visit_expression(tuple);
                }
                ExpressionKind::RefIsNull { value } => {
                    self.visit_expression(value);
                }
                ExpressionKind::RefEq { left, right } => {
                    self.visit_expression(left);
                    self.visit_expression(right);
                }
                ExpressionKind::RefAs { value, .. } => {
                    self.visit_expression(value);
                }
                ExpressionKind::TableGet { index, .. } => {
                    self.visit_expression(index);
                }
                ExpressionKind::TableSet { index, value, .. } => {
                    self.visit_expression(index);
                    self.visit_expression(value);
                }
                ExpressionKind::TableGrow { value, delta, .. } => {
                    self.visit_expression(value);
                    self.visit_expression(delta);
                }
                ExpressionKind::TableFill {
                    dest, value, size, ..
                } => {
                    self.visit_expression(dest);
                    self.visit_expression(value);
                    self.visit_expression(size);
                }
                ExpressionKind::TableCopy {
                    dest, src, size, ..
                } => {
                    self.visit_expression(dest);
                    self.visit_expression(src);
                    self.visit_expression(size);
                }
                ExpressionKind::TableInit {
                    dest, offset, size, ..
                } => {
                    self.visit_expression(dest);
                    self.visit_expression(offset);
                    self.visit_expression(size);
                }
                ExpressionKind::MemoryInit {
                    dest, offset, size, ..
                } => {
                    self.visit_expression(dest);
                    self.visit_expression(offset);
                    self.visit_expression(size);
                }
                ExpressionKind::MemoryCopy {
                    dest, src, size, ..
                } => {
                    self.visit_expression(dest);
                    self.visit_expression(src);
                    self.visit_expression(size);
                }
                ExpressionKind::MemoryFill {
                    dest, value, size, ..
                } => {
                    self.visit_expression(dest);
                    self.visit_expression(value);
                    self.visit_expression(size);
                }
                ExpressionKind::I31New { value } => {
                    self.visit_expression(value);
                }
                ExpressionKind::I31Get { i31, .. } => {
                    self.visit_expression(i31);
                }
                ExpressionKind::SIMDExtract { vec, .. } => {
                    self.visit_expression(vec);
                }
                ExpressionKind::SIMDReplace { vec, value, .. } => {
                    self.visit_expression(vec);
                    self.visit_expression(value);
                }
                ExpressionKind::SIMDShuffle { left, right, .. } => {
                    self.visit_expression(left);
                    self.visit_expression(right);
                }
                ExpressionKind::SIMDTernary { a, b, c, .. } => {
                    self.visit_expression(a);
                    self.visit_expression(b);
                    self.visit_expression(c);
                }
                ExpressionKind::SIMDShift { vec, shift, .. } => {
                    self.visit_expression(vec);
                    self.visit_expression(shift);
                }
                ExpressionKind::SIMDLoad { ptr, .. } => {
                    self.visit_expression(ptr);
                }
                ExpressionKind::SIMDLoadStoreLane { ptr, vec, .. } => {
                    self.visit_expression(ptr);
                    self.visit_expression(vec);
                }
                ExpressionKind::StructNew { operands, .. } => {
                    for e in operands.iter_mut() {
                        self.visit_expression(e);
                    }
                }
                ExpressionKind::StructGet { ptr, .. } => {
                    self.visit_expression(ptr);
                }
                ExpressionKind::StructSet { ptr, value, .. } => {
                    self.visit_expression(ptr);
                    self.visit_expression(value);
                }
                ExpressionKind::ArrayNew { size, init, .. } => {
                    self.visit_expression(size);
                    if let Some(i) = init {
                        self.visit_expression(i);
                    }
                }
                ExpressionKind::ArrayGet { ptr, index, .. } => {
                    self.visit_expression(ptr);
                    self.visit_expression(index);
                }
                ExpressionKind::ArraySet {
                    ptr, index, value, ..
                } => {
                    self.visit_expression(ptr);
                    self.visit_expression(index);
                    self.visit_expression(value);
                }
                ExpressionKind::ArrayLen { ptr } => {
                    self.visit_expression(ptr);
                }
                ExpressionKind::Try {
                    body, catch_bodies, ..
                } => {
                    self.visit_expression(body);
                    for e in catch_bodies.iter_mut() {
                        self.visit_expression(e);
                    }
                }
                ExpressionKind::Throw { operands, .. } => {
                    for e in operands.iter_mut() {
                        self.visit_expression(e);
                    }
                }
                ExpressionKind::RefTest { value, .. } => {
                    self.visit_expression(value);
                }
                ExpressionKind::RefCast { value, .. } => {
                    self.visit_expression(value);
                }
                ExpressionKind::BrOn { value, .. } => {
                    self.visit_expression(value);
                }
                // No children for these variants
                ExpressionKind::Const(_)
                | ExpressionKind::LocalGet { .. }
                | ExpressionKind::GlobalGet { .. }
                | ExpressionKind::Unreachable
                | ExpressionKind::Nop
                | ExpressionKind::AtomicFence
                | ExpressionKind::RefNull { .. }
                | ExpressionKind::RefFunc { .. }
                | ExpressionKind::TableSize { .. }
                | ExpressionKind::MemorySize { .. }
                | ExpressionKind::DataDrop { .. }
                | ExpressionKind::ElemDrop { .. }
                | ExpressionKind::Rethrow { .. }
                | ExpressionKind::Pop { .. } => {}
            }
        }

        // After visiting children, perform the copy coalescing logic for the current expression
        if let Some(expr_mut) = unsafe { expr.as_ptr().as_mut() } {
            if let ExpressionKind::LocalSet { id: set_id, value } = &mut expr_mut.kind {
                if let Some(value_expr_mut) = unsafe { value.as_ptr().as_mut() } {
                    if let ExpressionKind::LocalGet { id: get_id } = &value_expr_mut.kind {
                        // Check if the set index and get index are the same after remapping
                        // (Assuming LocalMapper has already run and updated indices)
                        if set_id == get_id {
                            // This is a redundant copy: local.set X (local.get X)
                            // Replace the LocalSet with a Drop of its value if it has a type, or a Nop if it's Type::NONE
                            // This effectively removes the redundant copy.
                            // The original type of the LocalSet becomes the type of the value (LocalGet's type)
                            // if it's not Type::NONE.
                            if expr_mut.type_ != Type::NONE {
                                expr_mut.kind = ExpressionKind::Drop { value: *value };
                            } else {
                                expr_mut.kind = ExpressionKind::Nop;
                            }
                            // The type of the expression should be updated to Type::NONE (if it's a Nop or Drop)
                            expr_mut.type_ = Type::NONE;
                        }
                    }
                }
            }
        }
    }
}

impl CoalesceLocals {
    pub fn run_on_func<'a>(func: &mut Function<'a>) {
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

        if let Some(body) = func.body_mut() {
            let builder = CFGBuilder::new();
            let mut cfg = builder.build(body);
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
            if let Some(body) = func.body_mut() {
                // Remap instructions
                let mut mapper = LocalMapper { mapping: &mapping };
                mapper.visit(body);

                // Remove dead stores after coalescing and remapping
                Self::eliminate_dead_stores(body);
            }

            // Remove merged locals from the arena
            for &old_id in mapping.keys() {
                func.locals_mut().remove_id(old_id);
            }

            if let Some(body) = func.body_mut() {
                // Perform copy coalescing after remapping and dead store elimination
                let mut copy_coalescer = CopyCoalescingVisitor;
                Visitor::visit(&mut copy_coalescer, body);
            }
        }
    }

    fn eliminate_dead_stores(root: &mut ExprRef) {
        let mut collector = UsedLocalCollector {
            used: HashSet::new(),
        };
        collector.visit(root);

        let mut eliminator = DeadStoreEliminator {
            used: &collector.used,
        };
        eliminator.visit(root);
    }
}

struct UsedLocalCollector {
    used: HashSet<crate::ir::LocalId>,
}
impl<'a> Visitor<'a> for UsedLocalCollector {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        if let ExpressionKind::LocalGet { id } = &expr.kind {
            self.used.insert(*id);
        }
    }
}

struct DeadStoreEliminator<'a> {
    used: &'a HashSet<crate::ir::LocalId>,
}
impl<'a, 'b> Visitor<'b> for DeadStoreEliminator<'a> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'b>) {
        match &mut expr.kind {
            ExpressionKind::LocalSet { id, value } | ExpressionKind::LocalTee { id, value } => {
                if !self.used.contains(id) {
                    expr.kind = ExpressionKind::Drop { value: *value };
                }
            }
            _ => {}
        }
    }
}
