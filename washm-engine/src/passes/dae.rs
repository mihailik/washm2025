use crate::ir::expression::ExprRef;
use crate::ir::module::Module;
use crate::passes::Pass;
use crate::ir::visitor::Visitor;

pub struct DAE;

impl Pass for DAE {
    fn name(&self) -> &str {
        "dae"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        for func in &mut module.functions {
            if let Some(body) = &mut func.body {
                let mut eliminator = DeadArgumentEliminator;
                eliminator.visit(body);
            }
        }
    }
}

struct DeadArgumentEliminator;
impl<'a> Visitor<'a> for DeadArgumentEliminator {
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
    fn test_dae() {
        let bump = Bump::new();
        let val = Expression::const_expr(&bump, Literal::I32(1), Type::I32);
        let func = Function::new("test".to_string(), Type::NONE, Type::I32, vec![], Some(val));
        let mut module = Module::new(&bump);
        module.add_function(func);
        let mut pass = DAE;
        pass.run(&mut module);
        assert!(module.functions[0].body.is_some());
    }
}
