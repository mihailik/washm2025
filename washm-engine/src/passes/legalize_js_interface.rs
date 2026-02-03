use crate::ir::expression::IrBuilder;
use crate::ir::module::{ExportKind, Function, Global, GlobalKind, Module};
use crate::ir::ops::{BinaryOp, UnaryOp};
use crate::ir::{Literal, LocalId, Type};
use crate::passes::Pass;

pub struct LegalizeJSInterface;

impl Pass for LegalizeJSInterface {
    fn name(&self) -> &str {
        "legalize-js-interface"
    }

    fn run(&mut self, module: &mut Module) {
        // LegalizeJSInterface
        // Goal: Transform exports/imports to be JS-compatible (handling i64).

        // 1. Check if we need to add tempRet0 global (for i64 results)
        let mut needs_temp_ret0 = false;
        for export in &module.exports {
            if let ExportKind::Function(id) = export.kind {
                if let Some(func) = module.functions.get(id.cast()) {
                    if func.results == Type::I64 {
                        needs_temp_ret0 = true;
                        break;
                    }
                }
            }
        }

        let temp_ret0_id = if needs_temp_ret0 {
            if let Some((id, _)) = module.get_global_by_name("tempRet0") {
                Some(id)
            } else {
                let (arena, _, _, globals, _, _, _, _, _, _, _, _, _) = module.parts_mut();
                let mut builder = IrBuilder::new(arena);
                let init = builder.const_(Literal::I32(0));
                Some(globals.alloc(Global {
                    name: "tempRet0".to_string(),
                    type_: Type::I32,
                    mutable: true,
                    kind: GlobalKind::Local { init },
                }))
            }
        } else {
            None
        };

        // 2. Process exports
        let mut exports_to_replace = Vec::new();
        for (i, export) in module.exports.iter().enumerate() {
            if let ExportKind::Function(id) = export.kind {
                if let Some(func) = module.functions.get(id.cast()) {
                    if func.params == Type::I64 || func.results == Type::I64 {
                        exports_to_replace.push((i, id.cast()));
                    }
                }
            }
        }

        for (export_idx, func_id) in exports_to_replace {
            let (old_params, old_results, func_name) = {
                let func = &module.functions[func_id];
                (func.params, func.results, func.name.clone())
            };

            let wrapper_name = format!("{}__js_wrapper", func_name);
            let mut wrapper_params_types = Vec::new();
            for param_type in old_params.tuple_elements() {
                if param_type == Type::I64 {
                    wrapper_params_types.push(Type::I32);
                    wrapper_params_types.push(Type::I32);
                } else {
                    wrapper_params_types.push(param_type);
                }
            }

            let wrapper_results = if old_results == Type::I64 {
                Type::I32
            } else {
                old_results
            };

            let mut current_local_idx = 0u32;
            let (wrapper_body, wrapper_params) = {
                let (arena, _, _, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
                let mut builder = IrBuilder::new(arena);
                let mut call_operands = Vec::new();

                for param_type in old_params.tuple_elements() {
                    if param_type == Type::I64 {
                        let low =
                            builder.local_get(LocalId::from_u32(current_local_idx), Type::I32);
                        let high =
                            builder.local_get(LocalId::from_u32(current_local_idx + 1), Type::I32);
                        let high64 = builder.unary(UnaryOp::ExtendUInt32, high, Type::I64);
                        let c32 = builder.const_(Literal::I64(32));
                        let shl = builder.binary(BinaryOp::ShlInt64, high64, c32, Type::I64);
                        let low64 = builder.unary(UnaryOp::ExtendUInt32, low, Type::I64);
                        call_operands.push(builder.binary(
                            BinaryOp::OrInt64,
                            low64,
                            shl,
                            Type::I64,
                        ));
                        current_local_idx += 2;
                    } else {
                        call_operands.push(
                            builder.local_get(LocalId::from_u32(current_local_idx), param_type),
                        );
                        current_local_idx += 1;
                    }
                }

                let call = builder.call(func_id, call_operands, old_results, false);

                let wrapper_body = if old_results == Type::I64 {
                    let temp_local = LocalId::from_u32(current_local_idx);
                    let mut list = Vec::new();
                    list.push(builder.local_set(temp_local, call));

                    if let Some(tr0) = temp_ret0_id {
                        let res_get = builder.local_get(temp_local, Type::I64);
                        let c32 = builder.const_(Literal::I64(32));
                        let shr = builder.binary(BinaryOp::ShrUInt64, res_get, c32, Type::I64);
                        let high32 = builder.unary(UnaryOp::WrapInt64, shr, Type::I32);
                        list.push(builder.global_set(tr0.cast(), high32));
                    }

                    let res_get2 = builder.local_get(temp_local, Type::I64);
                    let low32 = builder.unary(UnaryOp::WrapInt64, res_get2, Type::I32);
                    list.push(low32);
                    builder.block("", list, Type::I32)
                } else {
                    call
                };

                let wrapper_params = crate::ir::type_store::intern_tuple(wrapper_params_types);
                (wrapper_body, wrapper_params)
            };

            let (_, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
            let wrapper_func = Function::new(
                wrapper_name,
                wrapper_params,
                wrapper_results,
                if old_results == Type::I64 {
                    vec![Type::I64]
                } else {
                    vec![]
                },
                wrapper_body,
            );
            let wrapper_id = functions.alloc(wrapper_func);

            module.exports[export_idx].kind = ExportKind::Function(wrapper_id.cast());
        }
    }
}
