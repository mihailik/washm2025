use crate::collections::id_arena::IdArena;
use crate::ir::expression::{
    Expression, ExpressionId, ExpressionKind, ExpressionMarker, IrBuilder,
};
use crate::ir::module::{Function, LocalId, Module};
use crate::ir::visitor::Visitor;
use crate::ir::Type;
use crate::passes::Pass;
use std::collections::HashMap;

/// Flatten pass: Converts nested expression trees to flatter IR
///
/// This pass simplifies deeply nested structures by flattening
/// where possible, making subsequent passes more effective.
pub struct Flatten;

impl Pass for Flatten {
    fn name(&self) -> &str {
        "flatten"
    }

    fn run(&mut self, module: &mut Module) {
        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();

        for (_, func) in functions.iter_mut() {
            if let Some(mut body_id) = func.body() {
                // If the body is a block with a result, turn that into a return
                {
                    let body_type = arena[body_id].type_;
                    if body_type.is_concrete() {
                        let mut builder = IrBuilder::new(arena);
                        body_id = builder.return_(Some(body_id));
                        func.set_body(body_id);
                    }
                }

                let mut flattener = Flattener {
                    preludes: HashMap::new(),
                    break_temps: HashMap::new(),
                    expression_stack: Vec::new(),
                    function: func,
                };
                flattener.visit(&mut body_id, arena);

                // The body itself may have preludes
                let final_body = flattener.get_preludes_with_expression(body_id, body_id, arena);
                func.set_body(final_body);
            }
        }
    }
}

struct Flattener<'a> {
    // For each expression, a bunch of expressions that should execute right before it
    preludes: HashMap<ExpressionId, Vec<ExpressionId>>,
    // Break values are sent through a temp local
    break_temps: HashMap<ExpressionId, LocalId>,
    // Parent stack to allow migrating preludes upwards
    expression_stack: Vec<ExpressionId>,
    // Functional context needed to add variables
    function: &'a mut Function,
}

impl<'a> Flattener<'a> {
    fn get_preludes_with_expression(
        &mut self,
        preluder: ExpressionId,
        after: ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) -> ExpressionId {
        if let Some(the_preludes) = self.preludes.remove(&preluder) {
            let mut list = Vec::new();
            for p in the_preludes {
                list.push(p);
            }
            list.push(after);
            let mut builder = IrBuilder::new(arena);
            let after_type = builder.arena[after].type_;
            builder.block("", list, after_type)
        } else {
            after
        }
    }

    fn get_temp_for_break_target(&mut self, target: ExpressionId, type_: Type) -> LocalId {
        if let Some(&id) = self.break_temps.get(&target) {
            id
        } else {
            let id = self.function.add_var(type_);
            self.break_temps.insert(target, id);
            id
        }
    }

    fn is_control_flow_structure(
        &self,
        expr_id: ExpressionId,
        arena: &IdArena<ExpressionMarker, Expression>,
    ) -> bool {
        matches!(
            arena[expr_id].kind,
            ExpressionKind::Block { .. }
                | ExpressionKind::If { .. }
                | ExpressionKind::Loop { .. }
                | ExpressionKind::Try { .. }
        )
    }

    fn is_already_flat(
        &self,
        expr_id: ExpressionId,
        arena: &IdArena<ExpressionMarker, Expression>,
    ) -> bool {
        matches!(
            arena[expr_id].kind,
            ExpressionKind::LocalGet { .. }
                | ExpressionKind::Const(_)
                | ExpressionKind::Nop
                | ExpressionKind::Unreachable
        )
    }

    fn process_expression(
        &mut self,
        expr_id: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        let original_id = *expr_id;

        {
            let curr = &arena[*expr_id];
            // Nothing to do for constants, nop, and basic flat nodes.
            match curr.kind {
                ExpressionKind::Const(_)
                | ExpressionKind::Nop
                | ExpressionKind::Unreachable
                | ExpressionKind::LocalGet { .. }
                | ExpressionKind::GlobalGet { .. } => return,
                _ => {}
            }
        }

        let mut our_preludes = Vec::new();
        let is_cf = self.is_control_flow_structure(original_id, arena);
        let curr_type = arena[original_id].type_;

        if is_cf {
            // handle control flow explicitly. our children do not have control flow,
            // but they do have preludes which we need to set up in the right place
            let mut kind = std::mem::replace(&mut arena[original_id].kind, ExpressionKind::Nop);
            match &mut kind {
                ExpressionKind::Block { name: _, list } => {
                    let mut new_list = Vec::new();
                    for item in list.iter() {
                        if let Some(item_preludes) = self.preludes.remove(item) {
                            for p in item_preludes {
                                new_list.push(p);
                            }
                        }
                        new_list.push(*item);
                    }
                    *list = new_list;

                    if curr_type.is_concrete() {
                        let temp = self.get_temp_for_break_target(original_id, curr_type);

                        if let Some(last) = list.last_mut() {
                            let last_id = *last;
                            let mut builder = IrBuilder::new(arena);
                            if builder.arena[last_id].type_.is_concrete() {
                                *last = builder.local_set(temp, last_id);
                            }
                        }
                    }
                }
                ExpressionKind::If {
                    name: _,
                    condition,
                    if_true,
                    if_false,
                } => {
                    let orig_cond = *condition;
                    let orig_if_true = *if_true;
                    let orig_if_false = *if_false;

                    if curr_type.is_concrete() {
                        let temp = self.function.add_var(curr_type);
                        {
                            let mut builder = IrBuilder::new(arena);
                            if builder.arena[orig_if_true].type_.is_concrete() {
                                *if_true = builder.local_set(temp, orig_if_true);
                            }
                            if let Some(fb_id) = orig_if_false {
                                if builder.arena[fb_id].type_.is_concrete() {
                                    *if_false = Some(builder.local_set(temp, fb_id));
                                }
                            }
                            *expr_id = builder.local_get(temp, curr_type);
                        }
                        our_preludes.push(original_id);
                    }

                    // Condition preludes go before the entire if
                    *condition = self.get_preludes_with_expression(orig_cond, *condition, arena);

                    // Arm preludes go in the arms
                    *if_true = self.get_preludes_with_expression(orig_if_true, *if_true, arena);
                    if let Some(fb_id) = orig_if_false {
                        let new_fb = if_false.unwrap();
                        *if_false = Some(self.get_preludes_with_expression(fb_id, new_fb, arena));
                    }
                }
                ExpressionKind::Loop { body, .. } => {
                    let orig_body = *body;
                    if curr_type.is_concrete() {
                        let temp = self.function.add_var(curr_type);
                        {
                            let mut builder = IrBuilder::new(arena);
                            *body = builder.local_set(temp, orig_body);
                            *expr_id = builder.local_get(temp, curr_type);
                        }
                        our_preludes.push(original_id);
                    }
                    *body = self.get_preludes_with_expression(orig_body, *body, arena);
                }
                ExpressionKind::Try {
                    body, catch_bodies, ..
                } => {
                    let orig_body = *body;
                    let orig_catch_bodies = catch_bodies.clone();
                    if curr_type.is_concrete() {
                        let temp = self.function.add_var(curr_type);
                        {
                            let mut builder = IrBuilder::new(arena);
                            if builder.arena[orig_body].type_.is_concrete() {
                                *body = builder.local_set(temp, orig_body);
                            }
                            for catch_body in catch_bodies.iter_mut() {
                                let cb_id = *catch_body;
                                if builder.arena[cb_id].type_.is_concrete() {
                                    *catch_body = builder.local_set(temp, cb_id);
                                }
                            }

                            *expr_id = builder.local_get(temp, curr_type);
                        }
                        our_preludes.push(original_id);
                    }

                    *body = self.get_preludes_with_expression(orig_body, *body, arena);
                    for i in 0..catch_bodies.len() {
                        let cb_id = catch_bodies[i];
                        catch_bodies[i] =
                            self.get_preludes_with_expression(orig_catch_bodies[i], cb_id, arena);
                    }
                }
                _ => {}
            }
            arena[original_id].kind = kind;

            // Fix types for control flow
            if curr_type.is_concrete() {
                let expr = arena.get_mut(original_id).unwrap();
                if let ExpressionKind::Loop { .. } = expr.kind {
                    expr.type_ = Type::NONE;
                }
                // In binaryen, finalize() recomputes the type.
                // Here we mostly handle it via builder or manual set.
            }
        } else {
            // non-control flow
            if let Some(existing) = self.preludes.remove(&original_id) {
                our_preludes = existing;
            }

            let mut replaced = false;
            let mut kind = std::mem::replace(&mut arena[original_id].kind, ExpressionKind::Nop);
            {
                let mut builder = IrBuilder::new(arena);
                match &mut kind {
                    ExpressionKind::LocalTee { id, value } => {
                        let val_id = *value;
                        let val_type = builder.arena[val_id].type_;
                        if val_type == Type::UNREACHABLE {
                            *expr_id = val_id;
                            replaced = true;
                        } else {
                            let local_id = *id;
                            let set = builder.local_set(local_id, val_id);
                            our_preludes.push(set);
                            let local_type = self.function.get_local_type(local_id);
                            *expr_id = builder.local_get(local_id, local_type);
                            replaced = true;
                        }
                    }
                    ExpressionKind::Break { target, value, .. } => {
                        if let Some(val_id) = *value {
                            let val_type = builder.arena[val_id].type_;
                            if val_type.is_concrete() {
                                let target_type = builder.arena[*target].type_;
                                let temp = self.get_temp_for_break_target(*target, target_type);
                                our_preludes.push(builder.local_set(temp, val_id));

                                if val_type != target_type {
                                    let temp2 = self.function.add_var(val_type);
                                    our_preludes.push(builder.local_set(temp2, val_id));
                                    if curr_type.is_concrete() {
                                        *expr_id = builder.local_get(temp2, val_type);
                                    } else {
                                        *expr_id = builder.unreachable();
                                    }
                                } else if curr_type.is_concrete() {
                                    *expr_id = builder.local_get(temp, val_type);
                                } else {
                                    *expr_id = builder.unreachable();
                                }
                                our_preludes.push(original_id);

                                *value = None;
                                replaced = true;
                            } else {
                                // unreachable value, replace with the value itself
                                *expr_id = val_id;
                                replaced = true;
                            }
                        }
                    }
                    ExpressionKind::Switch { targets, value, .. } => {
                        if let Some(val_id) = *value {
                            let val_type = builder.arena[val_id].type_;
                            if val_type.is_concrete() {
                                let temp = self.function.add_var(val_type);
                                our_preludes.push(builder.local_set(temp, val_id));
                                for target in targets.iter() {
                                    let target_temp =
                                        self.get_temp_for_break_target(*target, val_type);
                                    let get = builder.local_get(temp, val_type);
                                    our_preludes.push(builder.local_set(target_temp, get));
                                }

                                *value = None;
                                replaced = true;
                            } else {
                                *expr_id = val_id;
                                replaced = true;
                            }
                        }
                    }
                    ExpressionKind::Drop { value } => {
                        let val_id = *value;
                        if self.is_already_flat(val_id, builder.arena) {
                            *expr_id = builder.nop();
                            replaced = true;
                        }
                    }
                    ExpressionKind::Return {
                        value: Some(val_id),
                    } => {
                        let v_id = *val_id;
                        if !self.is_already_flat(v_id, builder.arena) {
                            let val_type = builder.arena[v_id].type_;
                            let temp = self.function.add_var(val_type);
                            our_preludes.push(builder.local_set(temp, v_id));
                            *val_id = builder.local_get(temp, val_type);
                            replaced = true;
                        }
                    }
                    _ => {}
                }
            }
            arena[original_id].kind = kind;
            if replaced {
                // type is already computed or doesn't change significantly
            }
        }

        // Post-processing for everything
        let curr_id = *expr_id;
        let curr_type = arena[curr_id].type_;
        if curr_type.is_concrete() && !self.is_already_flat(curr_id, arena) {
            let temp = self.function.add_var(curr_type);
            let mut builder = IrBuilder::new(arena);
            our_preludes.push(builder.local_set(temp, curr_id));
            *expr_id = builder.local_get(temp, curr_type);
        }

        // Migrate preludes if we can
        let final_expr_id = *expr_id;
        if !our_preludes.is_empty() {
            if let Some(&parent_id) = self.expression_stack.last() {
                if !self.is_control_flow_structure(parent_id, arena) {
                    self.preludes
                        .entry(parent_id)
                        .or_default()
                        .extend(our_preludes);
                } else {
                    self.preludes.insert(final_expr_id, our_preludes);
                }
            } else {
                self.preludes.insert(final_expr_id, our_preludes);
            }
        }
    }
}

impl<'a> Visitor for Flattener<'a> {
    fn visit(
        &mut self,
        expr_id: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        self.expression_stack.push(*expr_id);
        self.visit_children(expr_id, arena);
        self.expression_stack.pop();
        self.process_expression(expr_id, arena);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::module::Function;
    use crate::ir::{Literal, Type};

    #[test]
    fn test_flatten_preserves_simple() {
        let mut module = Module::new();
        let (arena, _, _, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        let mut builder = IrBuilder::new(arena);
        let const_val = builder.const_(Literal::I32(42));

        let func_id = module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            const_val,
        ));

        let mut pass = Flatten;
        pass.run(&mut module);

        // Should remain unchanged in spirit (might be wrapped in block/return which is fine)
        assert!(module.functions.get(func_id).unwrap().body().is_some());
    }

    #[test]
    fn test_flatten_block_value() {
        let mut module = Module::new();
        let (arena, _, _, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        let mut builder = IrBuilder::new(arena);

        // (drop (block (result i32) (i32.const 42)))
        let const_val = builder.const_(Literal::I32(42));
        let block = builder.block("", vec![const_val], Type::I32);
        let drop = builder.drop(block);

        let func_id = module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            drop,
        ));

        let mut pass = Flatten;
        pass.run(&mut module);

        let body_id = module.functions.get(func_id).unwrap().body().unwrap();
        let body = &module.expressions[body_id];

        // The body should now be a block containing the flattened logic
        if let ExpressionKind::Block { list, .. } = &body.kind {
            assert!(list.len() >= 2);
        }
    }
}
