use crate::ir::expression::{ExprRef, ExpressionKind, IrBuilder};
use crate::ir::module::Module;
use crate::ir::ops::BinaryOp;
use crate::ir::visitor::Visitor;
use crate::ir::Literal;
use crate::passes::Pass;

/// Pick Load Signs pass: Optimizes load signs based on usage
///
/// adjust loads to signed/unsigned based on how they are used.
/// For example, if a signed load is masked to 8 bits, it can be unsigned.
pub struct PickLoadSigns;

impl Pass for PickLoadSigns {
    fn name(&self) -> &str {
        "pick-load-signs"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        let allocator = module.allocator;
        for func in &mut module.functions {
            if let Some(body) = &mut func.body {
                let mut picker = LoadSignPicker {
                    builder: IrBuilder::new(allocator),
                };
                picker.visit(body);
            }
        }
    }
}

#[allow(dead_code)]
struct LoadSignPicker<'a> {
    builder: IrBuilder<'a>,
}

impl<'a> Visitor<'a> for LoadSignPicker<'a> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        // Optimization: (i32.load8_s ...) & 0xFF -> i32.load8_u ...
        // Optimization: (i32.load16_s ...) & 0xFFFF -> i32.load16_u ...

        let mut replacement = None;

        if let ExpressionKind::Binary {
            op: BinaryOp::AndInt32,
            left,
            right,
            ..
        } = &mut expr.kind
        {
            if let ExpressionKind::Const(Literal::I32(mask)) = right.kind {
                if let ExpressionKind::Load { bytes, signed, .. } = &mut left.kind {
                    if *signed && ((*bytes == 1 && mask == 0xFF) || (*bytes == 2 && mask == 0xFFFF))
                    {
                        *signed = false;
                        // We capture the modified *left (which is an ExprRef).
                        // This ExprRef points to the Load expression (which we just modified in place).
                        replacement = Some(*left);
                    }
                }
            }
        }

        if let Some(new_expr) = replacement {
            *expr = new_expr;
        }

        self.visit_children(expr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::ExpressionKind;
    use crate::ir::module::Function;
    use crate::ir::{Literal, Type};
    use bumpalo::Bump;

    #[test]
    fn test_pick_load_signs_optimization() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // (i32.load8_s (i32.const 0)) & 0xFF
        let ptr = builder.const_(Literal::I32(0));
        let load = builder.load(1, true, 0, 1, ptr, Type::I32);
        let mask = builder.const_(Literal::I32(0xFF));
        let and = builder.binary(BinaryOp::AndInt32, load, mask, Type::I32);

        let func = Function::new("test".to_string(), Type::NONE, Type::I32, vec![], Some(and));
        let mut module = Module::new(&bump);
        module.add_function(func);

        let mut pass = PickLoadSigns;
        pass.run(&mut module);

        let body = module.functions[0].body.unwrap();

        // Should be just Load8U
        match body.kind {
            ExpressionKind::Load { bytes, signed, .. } => {
                assert_eq!(bytes, 1);
                assert!(!signed); // Changed to unsigned
            }
            ExpressionKind::Binary { .. } => {
                panic!("Should have removed the And");
            }
            _ => panic!("Expected Load"),
        }
    }
}
