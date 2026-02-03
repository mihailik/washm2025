use crate::collections::id_arena::IdArena;
use crate::ir::expression::{
    Expression, ExpressionId, ExpressionKind, ExpressionMarker, IrBuilder,
};
use crate::ir::module::Module;
use crate::ir::visitor::Visitor;
use crate::passes::Pass;

/// Untee pass: Converts local.tee operations back to local.set + local.get
///
/// This is useful when tees are not beneficial or when preparing for other optimizations
/// that work better with explicit sets and gets.
///
/// Transforms:
///   (local.tee $x (expr))
/// Into:
///   (block
///     (local.set $x (expr))
///     (local.get $x)
///   )
pub struct Untee;

impl Pass for Untee {
    fn name(&self) -> &str {
        "untee"
    }

    fn run(&mut self, module: &mut Module) {
        for (_, func) in &mut module.functions {
            if let Some(body) = func.body_mut() {
                let mut transformer = UnteeTransformer;
                transformer.visit(body, &mut module.expressions);
            }
        }
    }
}

struct UnteeTransformer;

impl Visitor for UnteeTransformer {
    fn visit_expression(
        &mut self,
        expr_id: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        let expr = &arena[*expr_id];
        if let ExpressionKind::LocalTee { id, value } = expr.kind {
            let tee_type = expr.type_;

            // Create: (block (local.set $x value) (local.get $x))
            let mut builder = IrBuilder::new(arena);
            let set = builder.local_set(id, value);
            let get = builder.local_get(id, tee_type);

            let list = vec![set, get];

            let new_block = builder.block("", list, tee_type);
            *expr_id = new_block;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::IrBuilder;
    use crate::ir::module::{Function, Module};
    use crate::ir::{Literal, LocalId, Type};

    #[test]
    fn test_untee_converts_tee_to_set_get() {
        let mut module = Module::new();

        // Create: (local.tee $0 (i32.const 42))
        let tee = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            let const_val = builder.const_(Literal::I32(42));
            builder.local_tee(LocalId::from(0), const_val, Type::I32)
        };

        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![Type::I32],
            tee,
        );

        let id = module.add_function(func);

        let mut pass = Untee;
        pass.run(&mut module);

        // Verify transformation
        let body_id = module.functions[id].body().unwrap();
        let body = &module.expressions[body_id];

        // Should now be a block
        if let ExpressionKind::Block { list, .. } = &body.kind {
            assert_eq!(list.len(), 2, "Expected block with 2 instructions");

            // First should be local.set
            let set_id = list[0];
            assert!(matches!(
                module.expressions[set_id].kind,
                ExpressionKind::LocalSet { .. }
            ));

            // Second should be local.get
            let get_id = list[1];
            assert!(matches!(
                module.expressions[get_id].kind,
                ExpressionKind::LocalGet { .. }
            ));
        } else {
            panic!("Expected block kind");
        }
    }
}
