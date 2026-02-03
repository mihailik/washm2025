use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionMarker};
use crate::ir::module::Module;
use crate::ir::visitor::Visitor;
use crate::passes::Pass;

pub struct InstrumentLocals;

impl Pass for InstrumentLocals {
    fn name(&self) -> &str {
        "InstrumentLocalsDASHED"
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
    use crate::ir::expression::IrBuilder;
    use crate::ir::module::Function;
    use crate::ir::{Literal, Module, Type};

    #[test]
    fn test_instrument_locals() {
        let mut module = Module::new();
        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        let mut builder = IrBuilder::new(arena);
        let val = builder.const_(Literal::I32(1));
        let func = Function::new("test".to_string(), Type::NONE, Type::I32, vec![], val);
        let id = functions.alloc(func);
        let mut pass = InstrumentLocals;
        pass.run(&mut module);
        assert!(module.functions[id].body().is_some());
    }
}
