use crate::analysis::effects::{Effect, EffectAnalyzer};
use crate::ir::expression::{ExprRef, Expression, ExpressionKind};
use crate::ir::module::Module;
use crate::ir::visitor::Visitor;
use crate::ir::Type;
use crate::passes::Pass;
use bumpalo::collections::Vec as BumpVec;
use std::collections::HashSet;

/// LICM (Loop-Invariant Code Motion): Hoists invariants out of loops
pub struct LICM;

impl Pass for LICM {
    fn name(&self) -> &str {
        "licm"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        let allocator = module.allocator();
        for func in &mut module.functions {
            if let Some(mut body) = func.body {
                let mut hoister = LoopInvariantHoister {
                    allocator,
                    vars: &mut func.vars,
                };
                hoister.visit(&mut body);
                func.body = Some(body);
            }
        }
    }
}

struct LoopInvariantHoister<'a, 'b> {
    allocator: &'a bumpalo::Bump,
    vars: &'b mut Vec<Type>,
}

#[derive(Default)]
struct LoopInfo {
    effects: Effect,
    modified_locals: HashSet<u32>,
}

impl<'a> Visitor<'a> for LoopInfo {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        let effect = EffectAnalyzer::analyze(*expr);
        self.effects |= effect;

        match &expr.kind {
            ExpressionKind::LocalSet { index, .. } | ExpressionKind::LocalTee { index, .. } => {
                self.modified_locals.insert(*index);
            }
            _ => {}
        }
        // Visitor trait will handle children
    }
}

#[derive(Default)]
struct LocalReadCollector {
    read_locals: HashSet<u32>,
}

impl<'a> Visitor<'a> for LocalReadCollector {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        if let ExpressionKind::LocalGet { index } = &expr.kind {
            self.read_locals.insert(*index);
        }
    }
}

impl<'a, 'b> Visitor<'a> for LoopInvariantHoister<'a, 'b> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        // First visit children in case there are nested loops
        self.visit_children(expr);

        if let ExpressionKind::Loop { name: _, body } = &mut expr.kind {
            // 1. Analyze the loop
            let mut info = LoopInfo::default();
            info.visit(body);

            // 2. Find and replace invariants in the loop body
            let mut replacer = InvariantReplacer {
                allocator: self.allocator,
                loop_info: &info,
                hoists: BumpVec::new_in(self.allocator),
                vars: self.vars,
            };
            replacer.visit(body);

            if !replacer.hoists.is_empty() {
                // 3. Wrap loop in a block with hoisted expressions
                let mut list = BumpVec::with_capacity_in(replacer.hoists.len() + 1, self.allocator);
                list.extend(replacer.hoists);
                list.push(*expr);

                let wrapper = Expression::block(self.allocator, None, list, expr.type_);
                *expr = wrapper;
            }
        }
    }
}

struct InvariantReplacer<'a, 'b, 'c> {
    allocator: &'a bumpalo::Bump,
    loop_info: &'b LoopInfo,
    hoists: BumpVec<'a, ExprRef<'a>>,
    vars: &'c mut Vec<Type>,
}

impl<'a, 'b, 'c> InvariantReplacer<'a, 'b, 'c> {
    fn is_invariant(&self, expr: ExprRef<'a>) -> bool {
        let effects = EffectAnalyzer::analyze(expr);

        // No side effects allowed
        if effects.intersects(Effect::SIDE_EFFECTS | Effect::MAY_TRAP) {
            return false;
        }

        // Check memory/global interference
        if effects.contains(Effect::MEMORY_READ)
            && self.loop_info.effects.contains(Effect::MEMORY_WRITE)
        {
            return false;
        }
        if effects.contains(Effect::GLOBAL_READ)
            && self.loop_info.effects.contains(Effect::GLOBAL_WRITE)
        {
            return false;
        }

        // Check local read/write interference
        let mut reader = LocalReadCollector::default();
        reader.visit(&mut { expr });
        for local in reader.read_locals {
            if self.loop_info.modified_locals.contains(&local) {
                return false;
            }
        }

        true
    }

    fn is_trivial(&self, expr: ExprRef<'a>) -> bool {
        match &expr.kind {
            ExpressionKind::Const(_) | ExpressionKind::LocalGet { .. } | ExpressionKind::Nop => {
                true
            }
            _ => false,
        }
    }
}

impl<'a, 'b, 'c> Visitor<'a> for InvariantReplacer<'a, 'b, 'c> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        if self.is_invariant(*expr) {
            if !self.is_trivial(*expr) {
                // Hoist it!
                let expr_type = expr.type_;
                let local_index = self.vars.len() as u32;
                self.vars.push(expr_type);

                let hoisted_expr = *expr;
                let set = Expression::local_set(self.allocator, local_index, hoisted_expr);
                self.hoists.push(set);

                let get = Expression::local_get(self.allocator, local_index, expr_type);
                *expr = get;
                return; // Don't visit children of hoisted expression
            }
        }

        self.visit_children(expr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::Expression;
    use crate::ir::{Literal, Type};
    use bumpalo::Bump;

    #[test]
    fn test_licm() {
        let bump = Bump::new();
        let val = Expression::const_expr(&bump, Literal::I32(1), Type::I32);
        let func = crate::ir::module::Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(val),
        );
        let mut module = Module::new(&bump);
        module.add_function(func);
        let mut pass = LICM;
        pass.run(&mut module);
        assert!(module.functions[0].body.is_some());
    }

    #[test]
    fn test_licm_hoist_binary() {
        let bump = Bump::new();
        let builder = crate::ir::expression::IrBuilder::new(&bump);

        // a, b are locals 0, 1. x is local 2.
        let a = builder.local_get(0, Type::I32);
        let b = builder.local_get(1, Type::I32);
        let add = builder.binary(crate::ir::ops::BinaryOp::AddInt32, a, b, Type::I32);
        let set_x = builder.local_set(2, add);

        let mut list = bumpalo::collections::Vec::new_in(&bump);
        list.push(set_x);
        let body = builder.block(None, list, Type::NONE);

        let loop_expr = builder.loop_(Some("loop"), body, Type::NONE);

        let func = crate::ir::module::Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![Type::I32, Type::I32, Type::I32],
            Some(loop_expr),
        );

        let mut module = Module::new(&bump);
        module.add_function(func);
        let mut pass = LICM;
        pass.run(&mut module);

        // After LICM, the body should be a block [hoist, loop]
        let body = module.functions[0].body.unwrap();
        if let ExpressionKind::Block { list, .. } = &body.kind {
            assert_eq!(list.len(), 2, "Should have hoisted one expression");
            // list[0] should be a local.set for the hoisted 'add'
            if let ExpressionKind::LocalSet { index, .. } = &list[0].kind {
                assert_eq!(*index, 3, "New local should be index 3");
            } else {
                panic!(
                    "First item in block should be a local.set, got {:?}",
                    list[0].kind
                );
            }
        } else {
            panic!("Body should be a block after hoisting, got {:?}", body.kind);
        }
    }
}
