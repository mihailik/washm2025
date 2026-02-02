use crate::ir::expression::{ExprRef, Expression, ExpressionKind};
use crate::ir::module::{ExportKind, Function, Global, GlobalKind, Module};
use crate::ir::ops::{BinaryOp, UnaryOp};
use crate::ir::{Literal, Type};
use crate::passes::Pass;
use bumpalo::collections::Vec as BumpVec;

pub struct LegalizeJSInterface;

impl Pass for LegalizeJSInterface {
    fn name(&self) -> &str {
        "legalize-js-interface"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        // LegalizeJSInterface
        // Goal: Transform exports/imports to be JS-compatible (handling i64).

        // 1. Check if we need to add tempRet0 global (for i64 results)
        // We only support single-value returns for now, so if any export returns i64, we need it.
        let mut needs_temp_ret0 = false;

        // Identify exports that need legalization
        struct ExportTask {
            export_index: usize, // Index in module.exports
            func_id: crate::ir::FuncId,
            _name: String,
            has_i64_param: bool,
            has_i64_result: bool,
        }

        let mut tasks = Vec::new();

        for (i, export) in module.exports.iter().enumerate() {
            if let ExportKind::Function(func_id) = export.kind {
                if let Some(func) = module.get_function(func_id) {
                    let has_i64_param = func.params == Type::I64; // Simple check for now
                    let has_i64_result = func.results == Type::I64;

                    if has_i64_param || has_i64_result {
                        if has_i64_result {
                            needs_temp_ret0 = true;
                        }
                        tasks.push(ExportTask {
                            export_index: i,
                            func_id,
                            _name: export.name.clone(),
                            has_i64_param,
                            has_i64_result,
                        });
                    }
                }
            }
        }

        if tasks.is_empty() {
            return;
        }

        // 2. Add tempRet0 global if needed
        let mut temp_ret0_id = None;
        if needs_temp_ret0 {
            // Check if it already exists
            if let Some((id, _)) = module.get_global_by_name("tempRet0") {
                temp_ret0_id = Some(id);
            } else {
                let init = module.allocator.alloc(Expression {
                    type_: Type::I32,
                    kind: ExpressionKind::Const(Literal::I32(0)),
                });
                let id = module.add_global(Global {
                    name: "tempRet0".to_string(),
                    type_: Type::I32,
                    mutable: true,
                    kind: GlobalKind::Local {
                        init: ExprRef::new(init),
                    },
                });
                temp_ret0_id = Some(id);
            }
        }

        // 3. Create wrapper functions
        let allocator = module.allocator;
        let mut new_funcs = Vec::new();

        // To avoid borrowing issues, we record the *new* function index for each task.
        for task in tasks.iter() {
            let original_func = &module.functions[task.func_id];
            let wrapper_name = format!("legalized_{}", original_func.name);

            // Signature of wrapper:
            // Params: if original takes i64, wrapper takes i32, i32.
            // Result: if original returns i64, wrapper returns i32.

            // Note: This logic assumes simple single-type signatures for now.
            // If Type is a tuple, we'd need to iterate it.

            let wrapper_params = if task.has_i64_param {
                // Placeholder: skipping param legalization for now due to Type limit
                Type::I64
            } else {
                original_func.params
            };

            let wrapper_results = if task.has_i64_result {
                Type::I32
            } else {
                original_func.results
            };

            // Build body
            // We need to call the original function.

            let mut args = BumpVec::new_in(allocator);
            // Assuming 1 param
            if original_func.params != Type::NONE {
                let get_local = allocator.alloc(Expression {
                    type_: original_func.params,
                    kind: ExpressionKind::LocalGet { index: 0 },
                });
                args.push(ExprRef::new(get_local));
            }

            let call_original = allocator.alloc(Expression {
                type_: original_func.results,
                kind: ExpressionKind::Call {
                    target: task.func_id,
                    operands: args,
                    is_return: false,
                },
            });

            let body = if task.has_i64_result {
                // Original returns i64. We need to split it.
                // We need to store the result in a local.

                // For simplified logic: param is index 0. temp var is index 0 (if no params) or 1.
                let temp_local_idx = if wrapper_params != Type::NONE { 1 } else { 0 };

                let mut block_list = BumpVec::new_in(allocator);

                // 1. local.set $temp (call)
                let set_temp = allocator.alloc(Expression {
                    type_: Type::NONE,
                    kind: ExpressionKind::LocalSet {
                        index: temp_local_idx,
                        value: ExprRef::new(call_original),
                    },
                });
                block_list.push(ExprRef::new(set_temp));

                // 2. global.set $tempRet0 (high bits)
                // i64.const 32
                let const_32 = allocator.alloc(Expression {
                    type_: Type::I64,
                    kind: ExpressionKind::Const(Literal::I64(32)),
                });
                // local.get $temp
                let get_temp_for_high = allocator.alloc(Expression {
                    type_: Type::I64,
                    kind: ExpressionKind::LocalGet {
                        index: temp_local_idx,
                    },
                });
                // i64.shr_u
                let shr = allocator.alloc(Expression {
                    type_: Type::I64,
                    kind: ExpressionKind::Binary {
                        op: BinaryOp::ShrUInt64,
                        left: ExprRef::new(get_temp_for_high),
                        right: ExprRef::new(const_32),
                    },
                });
                // i32.wrap
                let wrap_high = allocator.alloc(Expression {
                    type_: Type::I32,
                    kind: ExpressionKind::Unary {
                        op: UnaryOp::WrapInt64,
                        value: ExprRef::new(shr),
                    },
                });
                // global.set
                let set_global = allocator.alloc(Expression {
                    type_: Type::NONE,
                    kind: ExpressionKind::GlobalSet {
                        id: temp_ret0_id.unwrap(),
                        value: ExprRef::new(wrap_high),
                    },
                });
                block_list.push(ExprRef::new(set_global));

                // 3. i32.wrap (low bits) -> return value
                let get_temp_for_low = allocator.alloc(Expression {
                    type_: Type::I64,
                    kind: ExpressionKind::LocalGet {
                        index: temp_local_idx,
                    },
                });
                let wrap_low = allocator.alloc(Expression {
                    type_: Type::I32,
                    kind: ExpressionKind::Unary {
                        op: UnaryOp::WrapInt64,
                        value: ExprRef::new(get_temp_for_low),
                    },
                });
                block_list.push(ExprRef::new(wrap_low));

                allocator.alloc(Expression {
                    type_: Type::I32,
                    kind: ExpressionKind::Block {
                        name: None,
                        list: block_list,
                    },
                })
            } else {
                call_original
            };

            let vars = if task.has_i64_result {
                vec![Type::I64] // The temp var
            } else {
                vec![]
            };

            let wrapper_func = Function::new(
                wrapper_name,
                wrapper_params,
                wrapper_results,
                vars,
                Some(ExprRef::new(body)),
            );

            new_funcs.push(wrapper_func);
        }

        // 4. Update exports and add functions
        for (i, func) in new_funcs.into_iter().enumerate() {
            let func_id = module.add_function(func);

            // Update the export to point to this new function
            let task = &tasks[i];
            module.exports[task.export_index].kind = ExportKind::Function(func_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::{ExprRef, Expression, ExpressionKind};
    use crate::ir::module::Function;
    use crate::ir::FuncId;
    use crate::ir::{Literal, Type};

    use bumpalo::Bump;

    #[test]
    fn test_legalize_result() {
        let allocator = Bump::new();
        let mut module = Module::new(&allocator);

        // Create function that returns i64
        let const_val = allocator.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I64(0x123456789ABC)),
            type_: Type::I64,
        });

        let func = Function::new(
            "return_i64".to_string(),
            Type::NONE,
            Type::I64,
            vec![],
            Some(ExprRef::new(const_val)),
        );
        module.add_function(func);
        module.export_function(FuncId::from_raw_parts(0, 0), "return_i64".to_string());

        let mut pass = LegalizeJSInterface;
        pass.run(&mut module);

        // Verify:
        // 1. Export should point to a new function
        let export = &module.exports[0];
        let wrapper_idx = match export.kind {
            ExportKind::Function(idx) => idx,
            _ => panic!("Expected function export"),
        };
        assert_ne!(wrapper_idx, FuncId::from_raw_parts(0, 0));

        // 2. New function should return i32
        let wrapper = &module.functions[wrapper_idx];
        assert_eq!(wrapper.results, Type::I32);

        // 3. global tempRet0 should exist
        let g_id = module
            .globals
            .ids()
            .find(|id| module.globals[*id].name == "tempRet0")
            .expect("tempRet0 global not found");
        assert!(module.get_global(g_id.cast()).is_some());
    }
}
