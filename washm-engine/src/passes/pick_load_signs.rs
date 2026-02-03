use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
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

    fn run(&mut self, module: &mut Module) {
        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        for (_, func) in functions {
            if let Some(body) = func.body_mut() {
                let mut picker = LoadSignPicker;
                picker.visit(body, arena);
            }
        }
    }
}

struct LoadSignPicker;

impl Visitor for LoadSignPicker {
    fn visit_expression(
        &mut self,
        expr: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        // Optimization: (i32.load8_s ...) & 0xFF -> i32.load8_u ...
        // Optimization: (i32.load16_s ...) & 0xFFFF -> i32.load16_u ...

        let mut replacement = None;

        if let ExpressionKind::Binary {
            op: BinaryOp::AndInt32,
            left,
            right,
            ..
        } = &arena[*expr].kind
        {
            let left_id = *left;
            let right_id = *right;

            if let ExpressionKind::Const(Literal::I32(mask)) = arena[right_id].kind {
                if let ExpressionKind::Load { bytes, signed, .. } = &mut arena[left_id].kind {
                    if *signed && ((*bytes == 1 && mask == 0xFF) || (*bytes == 2 && mask == 0xFFFF))
                    {
                        *signed = false;
                        replacement = Some(left_id);
                    }
                }
            }
        }

        if let Some(new_expr) = replacement {
            *expr = new_expr;
        }

        self.visit_children(expr, arena);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::{ExpressionKind, IrBuilder};
    use crate::ir::module::Function;
    use crate::ir::{Literal, Type};

    #[test]
    fn test_pick_load_signs_optimization() {
        let mut module = Module::new();

        let mem_id = module
            .memories
            .alloc(crate::ir::module::Memory {
                name: "mem".to_string(),
                limits: crate::ir::module::MemoryLimits {
                    initial: 1,
                    maximum: None,
                },
                kind: crate::ir::module::MemoryKind::Local,
            })
            .cast();

        // (i32.load8_s (i32.const 0)) & 0xFF
        let mut builder = IrBuilder::new(&mut module.expressions);
        let ptr = builder.const_(Literal::I32(0));
        let load = builder.load(mem_id, 1, true, 0, 1, ptr, Type::I32);
        let mask = builder.const_(Literal::I32(0xFF));
        let and = builder.binary(BinaryOp::AndInt32, load, mask, Type::I32);

        let func = Function::new("test".to_string(), Type::NONE, Type::I32, vec![], and);
        let test_id = module.add_function(func);

        let mut pass = PickLoadSigns;
        pass.run(&mut module);

        let body_id = module
            .functions
            .get(test_id.cast())
            .unwrap()
            .body()
            .unwrap();
        let body = &module.expressions[body_id];

        // Should be just Load8U
        match &body.kind {
            ExpressionKind::Load { bytes, signed, .. } => {
                assert_eq!(*bytes, 1);
                assert!(!signed); // Changed to unsigned
            }
            ExpressionKind::Binary { .. } => {
                panic!("Should have removed the And");
            }
            _ => panic!("Expected Load"),
        }
    }
}
