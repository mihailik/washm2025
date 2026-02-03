use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::module::Module;
use crate::ir::visitor::Visitor;
use crate::ir::Type;
use crate::passes::Pass;

pub struct DCE;

impl Pass for DCE {
    fn name(&self) -> &str {
        "DCE"
    }

    fn run(&mut self, module: &mut Module) {
        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        for (_, func) in functions.iter_mut() {
            if let Some(body) = func.body_mut() {
                self.visit(body, arena);
            }
        }
    }
}

impl Visitor for DCE {
    fn visit(
        &mut self,
        expr_id: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        // First visit children
        self.visit_children(expr_id, arena);

        let kind = arena[*expr_id].kind.clone();
        if let ExpressionKind::Block { mut list, name } = kind {
            // Find the first instruction that doesn't return (is Unreachable)
            let mut cut_index = None;
            for (i, child_id) in list.iter().enumerate() {
                if arena[*child_id].type_ == Type::UNREACHABLE {
                    cut_index = Some(i + 1);
                    break;
                }
            }

            // If found, truncate the list
            if let Some(len) = cut_index {
                if len < list.len() {
                    list.truncate(len);
                    arena[*expr_id].kind = ExpressionKind::Block { list, name };
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::{ExpressionKind, IrBuilder};
    use crate::ir::module::Function;
    use crate::ir::{Literal, Module, Type};

    #[test]
    fn test_dce_removes_dead_code() {
        let mut module = Module::new();
        let mut builder = IrBuilder::new(&mut module.expressions);

        // Construct:
        // (block
        //    (i32.const 1)
        //    (unreachable)           <-- Has type UNREACHABLE
        //    (i32.const 3)           <-- Should be removed
        // )

        let const1 = builder.const_(Literal::I32(1));
        let terminator = builder.unreachable();
        let dead_code = builder.const_(Literal::I32(3));

        let block = builder.block(
            None::<String>,
            vec![const1, terminator, dead_code],
            Type::UNREACHABLE,
        );

        let func = Function::new("test".to_string(), Type::NONE, Type::NONE, vec![], block);

        module.add_function(func);

        let mut dce = DCE;
        dce.run(&mut module);

        let body_id = module.functions.iter().next().unwrap().1.body().unwrap();
        if let ExpressionKind::Block { list, .. } = &module.expressions[body_id].kind {
            assert_eq!(list.len(), 2);
            assert!(matches!(
                module.expressions[list[0]].kind,
                ExpressionKind::Const(Literal::I32(1))
            ));
            assert!(matches!(
                module.expressions[list[1]].kind,
                ExpressionKind::Unreachable
            ));
        } else {
            panic!("Expected block");
        }
    }
}
