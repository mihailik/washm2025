use crate::ir::expression::{ExprRef, ExpressionKind};
use crate::ir::module::Module;
use crate::ir::visitor::Visitor;
use crate::ir::Type;
use crate::passes::Pass;

pub struct DCE;

impl Pass for DCE {
    fn name(&self) -> &str {
        "DCE"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        for func in &mut module.functions {
            if let Some(body) = &mut func.body {
                self.visit(body);
            }
        }
    }
}

impl<'a> Visitor<'a> for DCE {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        if let ExpressionKind::Block { list, .. } = &mut expr.kind {
            // Find the first instruction that doesn't return (is Unreachable)
            let mut cut_index = None;
            for (i, child) in list.iter().enumerate() {
                if child.type_ == Type::UNREACHABLE {
                    cut_index = Some(i + 1);
                    break;
                }
            }

            // If found, truncate the list
            if let Some(len) = cut_index {
                if len < list.len() {
                    list.truncate(len);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::{ExprRef, Expression, ExpressionKind};
    use crate::ir::module::Function;
    use crate::ir::{Literal, Type};
    use bumpalo::collections::Vec as BumpVec;
    use bumpalo::Bump;

    #[test]
    fn test_dce_removes_dead_code() {
        let bump = Bump::new();

        // Construct:
        // (block
        //    (i32.const 1)
        //    (return (i32.const 2))  <-- Has type UNREACHABLE (simulated)
        //    (i32.const 3)           <-- Should be removed
        // )

        let const1 = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(1)),
            type_: Type::I32,
        }));

        // Simulate a helper that returns unreachable, e.g. an unconditional branch
        let terminator = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Nop,
            type_: Type::UNREACHABLE,
        }));

        let dead_code = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(3)),
            type_: Type::I32,
        }));

        let mut list = BumpVec::new_in(&bump);
        list.push(const1);
        list.push(terminator);
        list.push(dead_code);

        let block = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Block { name: None, list },
            type_: Type::UNREACHABLE, // Block is unreachable because it contains unreachable
        }));

        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(block),
        );

        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);
        module.add_function(func);

        let mut pass = DCE;
        pass.run(&mut module);

        let body = module.functions[0].body.as_ref().unwrap();
        if let ExpressionKind::Block { list, .. } = &body.kind {
            assert_eq!(list.len(), 2, "Expected 2 instructions after DCE");
            assert_eq!(list[0].type_, Type::I32);
            assert_eq!(list[1].type_, Type::UNREACHABLE);
        } else {
            panic!("Expected Block");
        }
    }
}
