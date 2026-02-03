use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::module::Module;
use crate::passes::Pass;

pub struct PostEmscripten;

impl Pass for PostEmscripten {
    fn name(&self) -> &str {
        "post-emscripten"
    }

    fn run(&mut self, module: &mut Module) {
        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        for (_, func) in functions {
            if let Some(body) = func.body() {
                optimize(body, arena);
            }
        }
    }
}

fn optimize(expr_id: ExpressionId, arena: &mut IdArena<ExpressionMarker, Expression>) {
    // Then optimize current expression
    // We do it before children for simplicity in this pass, or we can use a more robust visitor.
    let mut children = Vec::new();
    arena[expr_id].kind.for_each_child(|child| {
        children.push(child);
    });

    for child_id in children {
        optimize(child_id, arena);
    }

    if let ExpressionKind::Block { .. } = &arena[expr_id].kind {
        let mut list = if let ExpressionKind::Block { list, .. } = &mut arena[expr_id].kind {
            std::mem::take(list)
        } else {
            unreachable!()
        };

        optimize_block(&mut list, arena);

        if let ExpressionKind::Block {
            list: arena_list, ..
        } = &mut arena[expr_id].kind
        {
            *arena_list = list;
        }
    }
}

fn optimize_block(
    _list: &mut Vec<ExpressionId>,
    _arena: &mut IdArena<ExpressionMarker, Expression>,
) {
    // Placeholder for block-level optimizations
}
