use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::module::Module;
use crate::ir::visitor::Visitor;
use crate::ir::{Literal, Type};
use crate::passes::Pass;

pub struct OptimizeForJS;

impl Pass for OptimizeForJS {
    fn name(&self) -> &str {
        "optimize-for-js"
    }

    fn run(&mut self, module: &mut Module) {
        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        let mut optimizer = JSOptimizer;

        for (_, func) in functions {
            if let Some(mut body) = func.body_mut() {
                optimizer.visit(&mut body, arena);
            }
        }
    }
}

struct JSOptimizer;

impl Visitor for JSOptimizer {
    fn visit_expression(
        &mut self,
        expr_id: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        // 1. Unwrap singleton blocks with no name.
        if let ExpressionKind::Block { name, list } = &arena[*expr_id].kind {
            if name.is_none() && list.len() == 1 {
                let child = list[0];
                let (child_type, child_kind) = {
                    let child_node = &mut arena[child];
                    (
                        child_node.type_,
                        std::mem::replace(&mut child_node.kind, ExpressionKind::Nop),
                    )
                };
                arena[*expr_id].type_ = child_type;
                arena[*expr_id].kind = child_kind;
                return;
            }
        }

        // 2. Remove Drop of Const.
        if let ExpressionKind::Drop { value } = &arena[*expr_id].kind {
            if let ExpressionKind::Const(_) = arena[*value].kind {
                // Replace with Nop
                arena[*expr_id].kind = ExpressionKind::Nop;
                arena[*expr_id].type_ = Type::NONE;
                return;
            }
        }

        // 3. Optimize Select with constant condition.
        let mut replacement = None;
        if let ExpressionKind::Select {
            condition,
            if_true,
            if_false,
        } = &arena[*expr_id].kind
        {
            if let ExpressionKind::Const(lit) = arena[*condition].kind {
                let is_true = match lit {
                    Literal::I32(v) => v != 0,
                    Literal::I64(v) => v != 0,
                    Literal::F32(v) => v.to_bits() != 0,
                    Literal::F64(v) => v.to_bits() != 0,
                    _ => false,
                };
                replacement = Some(if is_true { *if_true } else { *if_false });
            }
        }
        if let Some(new_id) = replacement {
            *expr_id = new_id;
        }
    }
}
