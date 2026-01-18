use crate::ir::expression::{ExprRef, Expression, ExpressionKind};
use crate::ir::module::Module;
use crate::passes::Pass;
use bumpalo::collections::Vec as BumpVec;

pub struct PostEmscripten;

impl Pass for PostEmscripten {
    fn name(&self) -> &str {
        "post-emscripten"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        for func in &mut module.functions {
            if let Some(mut body) = func.body {
                optimize(&mut body);
            }
        }
    }
}

fn optimize<'a>(expr: &mut Expression<'a>) {
    match &mut expr.kind {
        ExpressionKind::Block { list, .. } => {
            for child in list.iter_mut() {
                optimize(child);
            }
            optimize_block(list);
        }
        ExpressionKind::If {
            condition,
            if_true,
            if_false,
        } => {
            optimize(condition);
            optimize(if_true);
            if let Some(if_false) = if_false {
                optimize(if_false);
            }
        }
        ExpressionKind::Loop { body, .. } => {
            optimize(body);
        }
        ExpressionKind::Unary { value, .. } => optimize(value),
        ExpressionKind::Binary { left, right, .. } => {
            optimize(left);
            optimize(right);
        }
        ExpressionKind::Call { operands, .. } => {
            for op in operands {
                optimize(op);
            }
        }
        ExpressionKind::LocalSet { value, .. } => optimize(value),
        ExpressionKind::LocalTee { value, .. } => optimize(value),
        ExpressionKind::GlobalSet { value, .. } => optimize(value),
        ExpressionKind::Load { ptr, .. } => optimize(ptr),
        ExpressionKind::Store { ptr, value, .. } => {
            optimize(ptr);
            optimize(value);
        }
        ExpressionKind::Return { value: Some(value) } => {
            optimize(value);
        }
        ExpressionKind::Drop { value } => optimize(value),
        ExpressionKind::Select {
            condition,
            if_true,
            if_false,
        } => {
            optimize(condition);
            optimize(if_true);
            optimize(if_false);
        }
        _ => {}
    }
}

fn optimize_block<'a>(_list: &mut BumpVec<'a, ExprRef<'a>>) {
    // Placeholder for block-level optimizations (stack save/restore removal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::{ExprRef, Expression, ExpressionKind};
    use crate::ir::module::Function;
    use crate::ir::Type;
    use bumpalo::collections::Vec as BumpVec;
    use bumpalo::Bump;

    #[test]
    fn test_post_emscripten_run() {
        let allocator = Bump::new();
        let mut module = Module::new(&allocator);

        let block = allocator.alloc(Expression {
            kind: ExpressionKind::Block {
                name: None,
                list: BumpVec::new_in(&allocator),
            },
            type_: Type::NONE,
        });

        let func = Function::new(
            "test_func".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(ExprRef::new(block)),
        );
        module.add_function(func);

        let mut pass = PostEmscripten;
        pass.run(&mut module);

        assert!(module.get_function("test_func").is_some());
    }
}
