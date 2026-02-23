use crate::ir::expression::IrBuilder;
use crate::ir::module::{Function, Module, TypeId};
use crate::ir::Type;
use crate::passes::Pass;
use std::collections::HashSet;

pub struct GenerateDynCalls;

impl Pass for GenerateDynCalls {
    fn name(&self) -> &str {
        "generate-dyncalls"
    }

    fn run(&mut self, module: &mut Module) {
        // Collect required signatures from Table elements
        let mut signatures: HashSet<(Type, Type)> = HashSet::new();

        for (_, segment) in &module.elements {
            for &func_idx in &segment.func_indices {
                if let Some(func) = module.functions.get(func_idx) {
                    // Currently we only support functions with NO parameters (Type::NONE)
                    // because we cannot represent multi-value params (tuples) in 'Function::params'
                    // easily without Type support.
                    if func.params == Type::NONE {
                        signatures.insert((func.params, func.results));
                    }
                }
            }
        }

        let table_id = module.tables.ids().next();

        // Generate wrappers
        let mut new_functions = Vec::new();

        for (params, results) in signatures {
            let name = generate_dyncall_name(params, results);

            if module.functions.iter().any(|(_, f)| f.name == name) {
                continue;
            }

            if let Some(table_id) = table_id {
                let wrapper = create_dyncall_wrapper(
                    &mut module.expressions,
                    &name,
                    params,
                    results,
                    table_id,
                );
                new_functions.push(wrapper);
            }
        }

        // Add to module and export
        for func in new_functions {
            let name = func.name.clone();
            let id = module.add_function(func);
            module.export_function(id, name);
        }
    }
}

fn generate_dyncall_name(params: Type, results: Type) -> String {
    let mut s = String::from("dynCall_");
    s.push(type_to_char(results));
    if params == Type::NONE {
        // Void params
    } else {
        s.push(type_to_char(params));
    }
    s
}

fn type_to_char(ty: Type) -> char {
    match ty {
        Type::NONE => 'v',
        Type::I32 => 'i',
        Type::I64 => 'j',
        Type::F32 => 'f',
        Type::F64 => 'd',
        Type::V128 => 'V',
        _ => 'X',
    }
}

fn create_dyncall_wrapper(
    arena: &mut crate::collections::id_arena::IdArena<
        crate::ir::expression::ExpressionMarker,
        crate::ir::expression::Expression,
    >,
    name: &str,
    _params: Type,
    results: Type,
    table: crate::ir::module::TableId,
) -> Function {
    let mut builder = IrBuilder::new(arena);

    // Wrapper params: Always (index: i32, ...original_params)
    // Since we only handle params == NONE, wrapper params is just I32.
    let wrapper_params = Type::I32;

    // Operands for CallIndirect
    let operands = Vec::new();

    // Target is the first argument (local 0)
    let target = builder.local_get(0, Type::I32);

    // The signature type for CallIndirect must be interned (params, results)
    // For now we just use a default TypeId until sig system is restored
    let type_id = TypeId::from_u32(0);

    let body = builder.call_indirect(table, target, operands, type_id, results, false);

    Function::new(
        name.to_string(),
        wrapper_params,
        results,
        vec![], // No locals
        body,
    )
}
