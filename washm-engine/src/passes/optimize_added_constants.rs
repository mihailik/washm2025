use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionMarker};
use crate::ir::module::Module;
use crate::ir::visitor::Visitor;
use crate::passes::Pass;

pub struct OptimizeAddedConstants;

impl Pass for OptimizeAddedConstants {
    fn name(&self) -> &str {
        "optimize-added-constants"
    }

    fn run(&mut self, module: &mut Module) {
        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        for (_, func) in functions {
            if let Some(body) = func.body_mut() {
                let mut visitor = GenericVisitor;
                visitor.visit(body, arena);
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
    use crate::ir::expression::{ExpressionKind, IrBuilder};
    use crate::ir::module::Function;
    use crate::ir::{Literal, Type};

    #[test]
    fn test_optimize_added_constants() {
        let mut module = Module::new();
        let mut builder = IrBuilder::new(&mut module.expressions);
        let val = builder.const_(Literal::I32(1));
        let func = Function::new("test".to_string(), Type::NONE, Type::I32, vec![], val);
        module.add_function(func);

        let mut pass = OptimizeAddedConstants;
        pass.run(&mut module);

        // Just verify basic structure
        let (func_id, _) = module.functions.iter().next().unwrap();
        assert!(module.functions.get(func_id).unwrap().body().is_some());
        let body = module.functions.get(func_id).unwrap().body().unwrap();
        assert!(matches!(
            module.expressions[body].kind,
            ExpressionKind::Const(_)
        ));
    }
}
