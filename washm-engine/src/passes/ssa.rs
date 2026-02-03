use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionMarker};
use crate::ir::module::Module;
use crate::ir::visitor::Visitor;
use crate::passes::Pass;

pub struct Ssa;

impl Pass for Ssa {
    fn name(&self) -> &str {
        "Ssa"
    }

    fn run(&mut self, module: &mut Module) {
        for (_, func) in &mut module.functions {
            if let Some(body) = func.body_mut() {
                let mut visitor = GenericVisitor;
                visitor.visit(body, &mut module.expressions);
            }
        }
    }
}

struct GenericVisitor;
impl Visitor for GenericVisitor {
    fn visit_expression(
        &mut self,
        _expr: &mut ExpressionId,
        _arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::{Expression, ExpressionKind};
    use crate::ir::module::{Function, Module};
    use crate::ir::{Literal, Type};

    #[test]
    fn test_ssa() {
        let mut module = Module::new();
        let val = module.expressions.alloc(Expression::new(
            ExpressionKind::Const(Literal::I32(1)),
            Type::I32,
        ));
        let func = Function::new("test".to_string(), Type::NONE, Type::I32, vec![], val);
        module.add_function(func);
        let mut pass = Ssa;
        pass.run(&mut module);
        assert!(module.functions[module.functions.iter().next().unwrap().0]
            .body()
            .is_some());
    }
}
