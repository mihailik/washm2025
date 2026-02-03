use crate::ir::expression::{ExpressionKind, IrBuilder};
use crate::ir::module::Module;
use crate::ir::ops::BinaryOp;
use crate::ir::Type;
use crate::passes::Pass;

pub struct StackCheck;

impl Pass for StackCheck {
    fn name(&self) -> &str {
        "stack-check"
    }

    fn run(&mut self, module: &mut Module) {
        // Find __stack_pointer and __stack_limit globals
        let mut sp_id = None;
        let mut limit_id = None;

        for (id, global) in module.globals.iter() {
            if global.name == "__stack_pointer" {
                sp_id = Some(id);
            } else if global.name == "__stack_limit" {
                limit_id = Some(id);
            }
        }

        if let (Some(sp), Some(limit)) = (sp_id, limit_id) {
            let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
            for (_, func) in functions.iter_mut() {
                if let Some(mut_body) = func.body_mut() {
                    let body_id = *mut_body;
                    let body_type = arena[body_id].type_;
                    let body_kind = arena[body_id].kind.clone();

                    let mut builder = IrBuilder::new(arena);
                    // Create check: if (sp < limit) unreachable
                    let sp_get = builder.global_get(sp, Type::I32);
                    let limit_get = builder.global_get(limit, Type::I32);
                    // Assuming stack grows down, check if SP < Limit (overflow)
                    // Using LtU32 for unsigned comparison of addresses
                    let condition =
                        builder.binary(BinaryOp::LtUInt32, sp_get, limit_get, Type::I32);
                    let trap = builder.unreachable();
                    let check = builder.if_("", condition, trap, None, Type::NONE);

                    // Prepend check to body
                    let new_body = match body_kind {
                        ExpressionKind::Block { name, mut list } => {
                            list.insert(0, check);
                            builder.block(name, list, body_type)
                        }
                        _ => {
                            let list = vec![check, body_id];
                            builder.block("", list, body_type)
                        }
                    };

                    *mut_body = new_body;
                }
            }
        }
    }
}
