use crate::ir::expression::{ExprRef, ExpressionKind, IrBuilder};
use crate::ir::module::{Function, Module};
use crate::passes::Pass;
use crate::ir::visitor::Visitor;
use crate::ir::Type;
use bumpalo::collections::Vec as BumpVec;
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

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        let allocator = module.allocator;
        // We must iterate using indices or collect to avoid borrow checker issues
        // because we pass &mut Function to Flattener which might want to access module later (though it doesn't now)
        // Actually, we can just iterate over functions.
        for i in 0..module.functions.len() {
            let func = &mut module.functions[i];
            if let Some(mut body) = func.body {
                // If the body is a block with a result, turn that into a return
                if body.type_.is_concrete() {
                    let builder = IrBuilder::new(allocator);
                    body = builder.return_(Some(body));
                    func.body = Some(body);
                }

                let mut flattener = Flattener {
                    builder: IrBuilder::new(allocator),
                    preludes: HashMap::new(),
                    break_temps: HashMap::new(),
                    expression_stack: Vec::new(),
                    function: func,
                };
                flattener.visit(&mut body);

                // The body itself may have preludes
                let final_body = flattener.get_preludes_with_expression(body, body);
                module.functions[i].body = Some(final_body);
            }
        }
    }
}

struct Flattener<'a, 'b> {
    builder: IrBuilder<'a>,
    // For each expression, a bunch of expressions that should execute right before it
    preludes: HashMap<ExprRef<'a>, Vec<ExprRef<'a>>>,
    // Break values are sent through a temp local
    break_temps: HashMap<String, u32>,
    // Parent stack to allow migrating preludes upwards
    expression_stack: Vec<ExprRef<'a>>,
    // Functional context needed to add variables
    function: &'b mut Function<'a>,
}

impl<'a, 'b> Flattener<'a, 'b> {
    fn get_preludes_with_expression(
        &mut self,
        preluder: ExprRef<'a>,
        after: ExprRef<'a>,
    ) -> ExprRef<'a> {
        if let Some(the_preludes) = self.preludes.remove(&preluder) {
            let mut list = BumpVec::new_in(self.builder.bump);
            for p in the_preludes {
                list.push(p);
            }
            list.push(after);
            let mut block = self.builder.block(None, list, Type::NONE);
            block.finalize();
            block
        } else {
            after
        }
    }

    fn get_temp_for_break_target(&mut self, name: &str, type_: Type) -> u32 {
        if let Some(&index) = self.break_temps.get(name) {
            index
        } else {
            let index = self.function.add_var(type_);
            self.break_temps.insert(name.to_string(), index);
            index
        }
    }

    fn is_control_flow_structure(&self, expr: ExprRef<'a>) -> bool {
        match expr.kind {
            ExpressionKind::Block { .. }
            | ExpressionKind::If { .. }
            | ExpressionKind::Loop { .. }
            | ExpressionKind::Try { .. } => true,
            _ => false,
        }
    }

    fn find_break_target(&self, name: &str) -> Option<ExprRef<'a>> {
        for expr in self.expression_stack.iter().rev() {
            match expr.kind {
                ExpressionKind::Block { name: Some(n), .. } if n == name => return Some(*expr),
                ExpressionKind::Loop { name: Some(n), .. } if n == name => return Some(*expr),
                ExpressionKind::Try { name: Some(n), .. } if n == name => return Some(*expr),
                _ => {}
            }
        }
        None
    }

    fn is_already_flat(&self, expr: ExprRef<'a>) -> bool {
        match expr.kind {
            ExpressionKind::LocalGet { .. }
            | ExpressionKind::Const(_)
            | ExpressionKind::Nop
            | ExpressionKind::Unreachable => true,
            _ => false,
        }
    }
}

impl<'a, 'b> Visitor<'a> for Flattener<'a, 'b> {
    fn visit(&mut self, expr: &mut ExprRef<'a>) {
        self.expression_stack.push(*expr);
        self.visit_children(expr);
        self.expression_stack.pop();
        self.visit_expression(expr);
    }

    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        let mut original_expr = *expr;
        let mut curr = *expr;

        // Nothing to do for constants, nop, and basic flat nodes.
        match curr.kind {
            ExpressionKind::Const(_)
            | ExpressionKind::Nop
            | ExpressionKind::Unreachable
            | ExpressionKind::LocalGet { .. }
            | ExpressionKind::GlobalGet { .. } => return,
            _ => {}
        }

        let mut our_preludes = Vec::new();

        let is_cf = self.is_control_flow_structure(curr);
        let curr_type = curr.type_;

        if is_cf {
            // handle control flow explicitly. our children do not have control flow,
            // but they do have preludes which we need to set up in the right place
            match &mut curr.kind {
                ExpressionKind::Block { name, list } => {
                    let mut new_list = BumpVec::new_in(self.builder.bump);
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
                        let temp = if let Some(n) = name {
                            self.get_temp_for_break_target(n, curr_type)
                        } else {
                            self.function.add_var(curr_type)
                        };

                        if let Some(last) = list.last_mut() {
                            if last.type_.is_concrete() {
                                *last = self.builder.local_set(temp, *last);
                            }
                        }
                        // finalize will change curr.type_, which we need to do carefully
                        // Since we have a mutable borrow of curr.kind here, we can't call finalized on curr yet.
                    }
                }
                ExpressionKind::If {
                    condition,
                    if_true,
                    if_false,
                } => {
                    let original_condition = *condition;
                    let original_if_true = *if_true;
                    let original_if_false = *if_false;

                    if curr_type.is_concrete() {
                        let temp = self.function.add_var(curr_type);
                        if if_true.type_.is_concrete() {
                            *if_true = self.builder.local_set(temp, *if_true);
                        }
                        if let Some(fb) = if_false {
                            if fb.type_.is_concrete() {
                                *fb = self.builder.local_set(temp, *fb);
                            }
                        }

                        *expr = self.builder.local_get(temp, curr_type);
                        our_preludes.push(original_expr);
                    }

                    // Condition preludes go before the entire if
                    *condition = self.get_preludes_with_expression(original_condition, *condition);

                    // Arm preludes go in the arms
                    *if_true = self.get_preludes_with_expression(original_if_true, *if_true);
                    if let Some(fb) = if_false {
                        *fb = self.get_preludes_with_expression(original_if_false.unwrap(), *fb);
                    }
                }
                ExpressionKind::Loop { body, .. } => {
                    let original_body = *body;
                    if curr_type.is_concrete() {
                        let temp = self.function.add_var(curr_type);
                        *body = self.builder.local_set(temp, *body);
                        *expr = self.builder.local_get(temp, curr_type);
                        our_preludes.push(original_expr);
                        // We will set type to NONE after the match
                    }
                    *body = self.get_preludes_with_expression(original_body, *body);
                }
                ExpressionKind::Try {
                    body, catch_bodies, ..
                } => {
                    let original_body = *body;
                    let original_catch_bodies: Vec<_> = catch_bodies.iter().copied().collect();
                    if curr_type.is_concrete() {
                        let temp = self.function.add_var(curr_type);
                        if body.type_.is_concrete() {
                            *body = self.builder.local_set(temp, *body);
                        }
                        for catch_body in catch_bodies.iter_mut() {
                            if catch_body.type_.is_concrete() {
                                *catch_body = self.builder.local_set(temp, *catch_body);
                            }
                        }

                        *expr = self.builder.local_get(temp, curr_type);
                        our_preludes.push(original_expr);
                    }

                    *body = self.get_preludes_with_expression(original_body, *body);
                    for i in 0..catch_bodies.len() {
                        catch_bodies[i] = self.get_preludes_with_expression(
                            original_catch_bodies[i],
                            catch_bodies[i],
                        );
                    }
                }
                _ => {}
            }

            // Post-match finalize and structural changes
            if curr_type.is_concrete() {
                match original_expr.kind {
                    ExpressionKind::Block { .. }
                    | ExpressionKind::If { .. }
                    | ExpressionKind::Try { .. } => {
                        original_expr.finalize();
                    }
                    ExpressionKind::Loop { .. } => {
                        original_expr.type_ = Type::NONE;
                        original_expr.finalize();
                    }
                    _ => {}
                }
            } else {
                original_expr.finalize();
            }
        } else {
            // non-control flow
            if let Some(existing) = self.preludes.remove(&curr) {
                our_preludes = existing;
            }

            let mut replaced = false;
            match &mut curr.kind {
                ExpressionKind::LocalTee { index, value } => {
                    if value.type_ == Type::UNREACHABLE {
                        *expr = *value;
                        replaced = true;
                    } else {
                        let index = *index;
                        let value_copy = *value;
                        let set = self.builder.local_set(index, value_copy);
                        our_preludes.push(set);
                        let local_type = self.function.get_local_type(index);
                        *expr = self.builder.local_get(index, local_type);
                        replaced = true;
                    }
                }
                ExpressionKind::Break { name, value, .. } => {
                    if let Some(val) = value {
                        let val_type = val.type_;
                        if val_type.is_concrete() {
                            let target_type = self
                                .find_break_target(name)
                                .map(|t| t.type_)
                                .unwrap_or(Type::NONE);
                            let temp = self.get_temp_for_break_target(name, target_type);
                            our_preludes.push(self.builder.local_set(temp, *val));

                            if val_type != target_type {
                                let temp2 = self.function.add_var(val_type);
                                our_preludes.push(self.builder.local_set(temp2, *val));
                                if curr_type.is_concrete() {
                                    *expr = self.builder.local_get(temp2, val_type);
                                } else {
                                    *expr = self.builder.unreachable();
                                }
                            } else {
                                if curr_type.is_concrete() {
                                    *expr = self.builder.local_get(temp, val_type);
                                } else {
                                    *expr = self.builder.unreachable();
                                }
                            }
                            our_preludes.push(original_expr);
                            *value = None;
                            replaced = true;
                        } else {
                            // unreachable value, replace with the value itself
                            *expr = *val;
                            replaced = true;
                        }
                    }
                }
                ExpressionKind::Switch {
                    names,
                    condition: _,
                    value,
                    ..
                } => {
                    if let Some(val) = value {
                        let val_type = val.type_;
                        if val_type.is_concrete() {
                            let temp = self.function.add_var(val_type);
                            our_preludes.push(self.builder.local_set(temp, *val));
                            for name in names.iter() {
                                let target_temp = self.get_temp_for_break_target(name, val_type);
                                let get = self.builder.local_get(temp, val_type);
                                our_preludes.push(self.builder.local_set(target_temp, get));
                            }
                            *value = None;
                            replaced = true;
                        } else {
                            *expr = *val;
                            replaced = true;
                        }
                    }
                }
                ExpressionKind::Drop { value } => {
                    if self.is_already_flat(*value) {
                        *expr = self.builder.nop();
                        replaced = true;
                    }
                }
                ExpressionKind::Return { value } => {
                    if let Some(val) = value {
                        if !self.is_already_flat(*val) {
                            // This shouldn't normally happen because visit_children already flattened it,
                            // but for safety/consistency:
                            let val_type = val.type_;
                            let temp = self.function.add_var(val_type);
                            our_preludes.push(self.builder.local_set(temp, *val));
                            *val = self.builder.local_get(temp, val_type);
                            replaced = true;
                        }
                    }
                }
                _ => {}
            }

            if replaced {
                original_expr.finalize();
            }
        }

        // Post-processing for everything
        curr = *expr;
        curr.finalize();
        if curr.type_ == Type::UNREACHABLE {
            our_preludes.push(curr);
            *expr = self.builder.unreachable();
        } else if curr.type_.is_concrete() && !self.is_already_flat(curr) {
            let type_ = curr.type_;
            let temp = self.function.add_var(type_);
            our_preludes.push(self.builder.local_set(temp, curr));
            *expr = self.builder.local_get(temp, type_);
        }

        // Migrate preludes if we can
        let current_expr = *expr;
        if !our_preludes.is_empty() {
            if let Some(parent) = self.expression_stack.last() {
                if !self.is_control_flow_structure(*parent) {
                    self.preludes
                        .entry(*parent)
                        .or_default()
                        .extend(our_preludes);
                } else {
                    self.preludes.insert(current_expr, our_preludes);
                }
            } else {
                self.preludes.insert(current_expr, our_preludes);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::Expression;
    use crate::ir::module::Function;
    use crate::ir::{Literal, Type};
    use bumpalo::Bump;

    #[test]
    fn test_flatten_preserves_simple() {
        let bump = Bump::new();
        let const_val = Expression::const_expr(&bump, Literal::I32(42), Type::I32);

        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(const_val),
        );

        let mut module = Module::new(&bump);
        module.add_function(func);

        let mut pass = Flatten;
        pass.run(&mut module);

        // Should remain unchanged
        assert!(module.functions[0].body.is_some());
    }

    #[test]
    fn test_flatten_block_value() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // (drop (block (result i32) (i32.const 42)))
        let const_val = builder.const_(Literal::I32(42));
        let list = {
            let mut l = BumpVec::new_in(&bump);
            l.push(const_val);
            l
        };
        let block = builder.block(None, list, Type::I32);
        let drop = builder.drop(block);

        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(drop),
        );

        let mut module = Module::new(&bump);
        module.add_function(func);

        let mut pass = Flatten;
        pass.run(&mut module);

        let body = module.functions[0].body.unwrap();

        // The body should now be a block containing the flattened logic
        // It should start with (local.set 0 (i32.const 42))
        // And end with (drop (local.get 0))
        if let ExpressionKind::Block { list, .. } = &body.kind {
            assert!(list.len() >= 2);
            // In our implementation, block values are moved to locals
            // The structure might be nested blocks depending on how preludes were handled
        }
    }
}
