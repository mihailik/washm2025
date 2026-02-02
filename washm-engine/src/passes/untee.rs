use crate::ir::expression::{ExprRef, Expression, ExpressionKind};
use crate::ir::module::Module;
use crate::ir::visitor::Visitor;
use crate::passes::Pass;
use bumpalo::collections::Vec as BumpVec;

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

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        let allocator = module.allocator();

        for (_, func) in &mut module.functions {
            if let Some(body) = func.body_mut() {
                let mut transformer = UnteeTransformer { allocator };
                transformer.visit(body);
            }
        }
    }
}

struct UnteeTransformer<'a> {
    allocator: &'a bumpalo::Bump,
}

impl<'a> Visitor<'a> for UnteeTransformer<'a> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        if let ExpressionKind::LocalTee { index, value } = &expr.kind {
            let local_index = *index;
            let tee_value = *value;
            let tee_type = expr.type_;

            // Create: (block (local.set $x value) (local.get $x))
            let set = Expression::local_set(self.allocator, local_index, tee_value);
            let get = Expression::local_get(self.allocator, local_index, tee_type);

            let mut list = BumpVec::new_in(self.allocator);
            list.push(set);
            list.push(get);

            *expr = Expression::block(self.allocator, None, list, tee_type);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::{Expression, ExpressionKind};
    use crate::ir::module::Function;
    use crate::ir::{Literal, Type};
    use bumpalo::Bump;

    #[test]
    fn test_untee_converts_tee_to_set_get() {
        let bump = Bump::new();

        // Create: (local.tee $0 (i32.const 42))
        let const_val = Expression::const_expr(&bump, Literal::I32(42), Type::I32);
        let tee = Expression::local_tee(&bump, 0, const_val, Type::I32);

        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![Type::I32],
            Some(tee),
        );

        let mut module = Module::new(&bump);
        let id = module.add_function(func);

        let mut pass = Untee;
        pass.run(&mut module);

        // Verify transformation
        let binding = module.functions[id].body();
        let body = binding.as_ref().unwrap();

        // Should now be a block
        if let ExpressionKind::Block { list, .. } = &body.kind {
            assert_eq!(list.len(), 2, "Expected block with 2 instructions");

            // First should be local.set
            if let ExpressionKind::LocalSet { index, .. } = &list[0].kind {
                assert_eq!(*index, 0);
            } else {
                panic!("Expected LocalSet, got {:?}", list[0].kind);
            }

            // Second should be local.get
            if let ExpressionKind::LocalGet { index } = &list[1].kind {
                assert_eq!(*index, 0);
            } else {
                panic!("Expected LocalGet, got {:?}", list[1].kind);
            }
        } else {
            panic!("Expected Block, got {:?}", body.kind);
        }
    }

    #[test]
    fn test_untee_preserves_non_tee() {
        let bump = Bump::new();

        // Create: (local.get $0)
        let get = Expression::local_get(&bump, 0, Type::I32);

        let func = Function::new(
            "test".to_string(),
            Type::I32,
            Type::I32,
            vec![Type::I32],
            Some(get),
        );

        let mut module = Module::new(&bump);
        let id = module.add_function(func);

        let mut pass = Untee;
        pass.run(&mut module);

        // Should remain unchanged
        let binding = module.functions[id].body();
        let body = binding.as_ref().unwrap();
        assert!(matches!(body.kind, ExpressionKind::LocalGet { .. }));
    }

    #[test]
    fn test_untee_preserves_type() {
        let bump = Bump::new();

        // Create: (local.tee $0 (i64.const 100))
        let const_val = Expression::const_expr(&bump, Literal::I64(100), Type::I64);
        let tee = Expression::local_tee(&bump, 0, const_val, Type::I64);

        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I64,
            vec![Type::I64],
            Some(tee),
        );

        let mut module = Module::new(&bump);
        let id = module.add_function(func);

        let mut pass = Untee;
        pass.run(&mut module);

        // Verify type is preserved
        let binding = module.functions[id].body();
        let body = binding.as_ref().unwrap();
        assert_eq!(body.type_, Type::I64, "Block should preserve tee's type");
    }
}
