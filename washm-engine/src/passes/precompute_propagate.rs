use crate::analysis::evaluator::Evaluator;
use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::module::Module;
use crate::ir::visitor::Visitor;
use crate::ir::Literal;
use crate::passes::Pass;
use std::collections::HashMap;

/// PrecomputePropagate pass: Evaluates expressions and propagates constants through locals
pub struct PrecomputePropagate;

impl Pass for PrecomputePropagate {
    fn name(&self) -> &str {
        "precompute-propagate"
    }

    fn run(&mut self, module: &mut Module) {
        let evaluator = Evaluator::new();
        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();

        for (_, func) in functions {
            if let Some(body) = func.body_mut() {
                // We need to run this until convergence or a fixed number of times
                // since propagating a constant might enable further folding
                for _ in 0..3 {
                    let mut visitor = PrecomputePropagateVisitor::new(&evaluator);
                    visitor.visit(body, arena);
                    if !visitor.made_changes {
                        break;
                    }
                }
            }
        }
    }
}

struct PrecomputePropagateVisitor<'a> {
    evaluator: &'a Evaluator,
    /// Known values for locals in the current control flow scope
    /// Map from Local ID -> Literal
    known_locals: HashMap<crate::ir::LocalId, Literal>,
    made_changes: bool,
}

impl<'a> PrecomputePropagateVisitor<'a> {
    fn new(evaluator: &'a Evaluator) -> Self {
        Self {
            evaluator,
            known_locals: HashMap::new(),
            made_changes: false,
        }
    }

    fn invalidate_all(&mut self) {
        self.known_locals.clear();
    }
}

impl<'a> Visitor for PrecomputePropagateVisitor<'a> {
    fn visit(
        &mut self,
        expr_id: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        // Pre-order traversal for control flow handling
        match &arena[*expr_id].kind {
            // Loops invalidate our simple straight-line knowledge because of back-edges
            ExpressionKind::Loop { .. }
            | ExpressionKind::Break { .. }
            | ExpressionKind::Switch { .. } => {
                self.invalidate_all();
            }
            // If handling: ideally we branch and merge.
            // For now, let's just invalidate when entering an If to keep it simple but safe.
            ExpressionKind::If { .. } => {
                self.invalidate_all();
            }
            _ => {}
        }

        // Visit children first (bottom-up for expression folding)
        self.visit_children(expr_id, arena);

        // Post-order processing
        match &arena[*expr_id].kind {
            ExpressionKind::LocalSet { id, value } | ExpressionKind::LocalTee { id, value } => {
                let id = *id;
                let value_id = *value;
                // If value is a constant, record it
                if let ExpressionKind::Const(lit) = arena[value_id].kind {
                    self.known_locals.insert(id, lit);
                } else {
                    // Otherwise, we don't know this local anymore
                    self.known_locals.remove(&id);
                }
            }
            ExpressionKind::LocalGet { id } => {
                // If we know the value, replace with constant
                if let Some(lit) = self.known_locals.get(id) {
                    arena[*expr_id].kind = ExpressionKind::Const(*lit);
                    self.made_changes = true;
                }
            }
            _ => {
                // Try to evaluate the expression
                if let Some(lit) = self.evaluator.eval(*expr_id, arena) {
                    // Only replace if it wasn't already a constant
                    if !matches!(arena[*expr_id].kind, ExpressionKind::Const(_)) {
                        arena[*expr_id].kind = ExpressionKind::Const(lit);
                        self.made_changes = true;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::ExpressionKind;
    use crate::ir::expression::IrBuilder;
    use crate::ir::module::Function;
    use crate::ir::ops::BinaryOp;
    use crate::ir::{Literal, LocalId, Type};

    #[test]
    fn test_propagate_basic() {
        let mut module = Module::new();

        let body = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            let const42 = builder.const_(Literal::I32(42));
            let set = builder.local_set(LocalId::from(0), const42);
            let get = builder.local_get(LocalId::from(0), Type::I32);
            let const10 = builder.const_(Literal::I32(10));
            let add = builder.binary(BinaryOp::AddInt32, get, const10, Type::I32);
            builder.block("", vec![set, add], Type::I32)
        };

        let func = Function::new(
            "test".to_string(),
            Type::NONE,      // params
            Type::I32,       // results
            vec![Type::I32], // vars (locals)
            body,
        );
        module.add_function(func);

        let mut pass = PrecomputePropagate;
        pass.run(&mut module);

        // Check if the add was folded to 52
        let func_id = module.functions.iter().next().unwrap().0;
        let func = &module.functions[func_id];
        let body_id = func.body().unwrap();

        if let ExpressionKind::Block { list, .. } = &module.expressions[body_id].kind {
            let last = list.last().unwrap();
            assert!(matches!(
                module.expressions[*last].kind,
                ExpressionKind::Const(Literal::I32(52))
            ));
        } else {
            panic!("Expected block");
        }
    }
}
