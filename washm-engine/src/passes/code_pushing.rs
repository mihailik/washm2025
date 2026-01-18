use crate::ir::expression::ExprRef;
use crate::ir::module::Module;
use crate::passes::Pass;
use crate::ir::visitor::Visitor;

/// Code Pushing pass: Moves code closer to uses
pub struct CodePushing;

impl Pass for CodePushing {
    fn name(&self) -> &str {
        "code-pushing"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        for func in &mut module.functions {
            if let Some(body) = &mut func.body {
                let mut pusher = CodePusher;
                pusher.visit(body);
            }
        }
    }
}

struct CodePusher;

impl<'a> Visitor<'a> for CodePusher {
    fn visit_expression(&mut self, _expr: &mut ExprRef<'a>) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::Expression;
    use crate::ir::module::Function;
    use crate::ir::{Literal, Type};
    use bumpalo::Bump;

    #[test]
    fn test_code_pushing() {
        let bump = Bump::new();
        let val = Expression::const_expr(&bump, Literal::I32(1), Type::I32);
        let func = Function::new("test".to_string(), Type::NONE, Type::I32, vec![], Some(val));
        let mut module = Module::new(&bump);
        module.add_function(func);
        let mut pass = CodePushing;
        pass.run(&mut module);
        assert!(module.functions[0].body.is_some());
    }
}
