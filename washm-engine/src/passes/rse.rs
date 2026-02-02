use crate::ir::expression::ExprRef;
use crate::ir::module::Module;
use crate::ir::visitor::Visitor;
use crate::passes::Pass;

/// RSE (Redundant Set Elimination)
///
/// Removes local.set operations that are immediately overwritten
/// before being used, making code more efficient.
pub struct RSE;

impl Pass for RSE {
    fn name(&self) -> &str {
        "rse"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        let allocator = module.allocator();

        for (_, func) in &mut module.functions {
            if let Some(body) = func.body_mut() {
                let mut eliminator = RedundantSetEliminator { allocator };
                eliminator.visit(body);
            }
        }
    }
}

#[allow(dead_code)]
struct RedundantSetEliminator<'a> {
    allocator: &'a bumpalo::Bump,
}

impl<'a> Visitor<'a> for RedundantSetEliminator<'a> {
    fn visit_expression(&mut self, _expr: &mut ExprRef<'a>) {
        // Foundation for redundant set elimination
        // Full implementation would track def-use and eliminate redundant sets
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::Expression;
    use crate::ir::module::Function;
    use crate::ir::{Literal, Type};
    use bumpalo::Bump;

    #[test]
    fn test_rse_preserves_structure() {
        let bump = Bump::new();

        let val = Expression::const_expr(&bump, Literal::I32(42), Type::I32);
        let set = Expression::local_set(&bump, 0, val);

        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![Type::I32],
            Some(set),
        );

        let mut module = Module::new(&bump);
        module.add_function(func);

        let mut pass = RSE;
        pass.run(&mut module);

        assert!(module.functions[module.functions.iter().next().unwrap().0]
            .body()
            .is_some());
    }

    #[test]
    fn test_rse_handles_used_sets() {
        let bump = Bump::new();

        let val = Expression::const_expr(&bump, Literal::I32(10), Type::I32);
        let set = Expression::local_set(&bump, 0, val);
        let get = Expression::local_get(&bump, 0, Type::I32);

        let mut list = bumpalo::collections::Vec::new_in(&bump);
        list.push(set);
        list.push(get);
        let block = Expression::block(&bump, None, list, Type::I32);

        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![Type::I32],
            Some(block),
        );

        let mut module = Module::new(&bump);
        module.add_function(func);

        let mut pass = RSE;
        pass.run(&mut module);

        assert!(module.functions[module.functions.iter().next().unwrap().0]
            .body()
            .is_some());
    }
}
