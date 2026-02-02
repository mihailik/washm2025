use crate::ir::expression::{Expression, ExpressionKind, IrBuilder};
use crate::ir::module::{Function, Module};
use crate::ir::Type;
use crate::passes::Pass;

use bumpalo::collections::Vec as BumpVec;
use std::collections::HashSet;

pub struct GenerateDynCalls;

impl Pass for GenerateDynCalls {
    fn name(&self) -> &str {
        "generate-dyncalls"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        // Collect required signatures from Table elements
        let mut signatures: HashSet<(Type, Type)> = HashSet::new();

        for segment in &module.elements {
            for &func_idx in &segment.func_indices {
                if let Some(func) = module.functions.get(func_idx) {
                    // Currently we only support functions with NO parameters (Type::NONE)
                    // because we cannot represent multi-value params (tuples) in 'Function::params'
                    // easily without Type support.
                    // The wrapper needs to take (index: i32, ...params).
                    // If params is NONE, wrapper params is I32 (just the index).
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
                let wrapper =
                    create_dyncall_wrapper(module.allocator, &name, params, results, table_id);
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

fn create_dyncall_wrapper<'a>(
    bump: &'a bumpalo::Bump,
    name: &str,
    params: Type,
    results: Type,
    table: crate::ir::module::TableId,
) -> Function<'a> {
    let builder = IrBuilder::new(bump);

    // Wrapper params: Always (index: i32, ...original_params)
    // Since we only handle params == NONE, wrapper params is just I32.
    let wrapper_params = Type::I32;

    // Operands for CallIndirect:
    // [original_params...]
    // Since params == NONE, no operands other than implicit target?
    // CallIndirect takes: target (index), operands.
    let operands = BumpVec::new_in(bump);

    // Target is the first argument (local 0)
    let target = builder.local_get(0, Type::I32);

    // The signature type for CallIndirect must be interned (params, results)
    let sig_type = crate::ir::type_store::intern_signature(params, results);

    let body = Expression::new(
        bump,
        ExpressionKind::CallIndirect {
            table,
            target,
            operands,
            type_: sig_type,
        },
        results,
    );

    Function::new(
        name.to_string(),
        wrapper_params,
        results,
        vec![], // No locals
        Some(body),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::{ExprRef, Expression, ExpressionKind};
    use crate::ir::module::{ElementSegment, Function};
    use crate::ir::TableId;
    use crate::ir::Type;
    use bumpalo::collections::Vec as BumpVec;
    use bumpalo::Bump;

    #[test]
    fn test_generate_dyncalls_void_void() {
        let allocator = Bump::new();
        let mut module = Module::new(&allocator);

        // Add a table
        module.add_table(crate::ir::module::Table {
            name: "table".to_string(),
            limits: crate::ir::module::TableLimits {
                initial: 1,
                maximum: None,
                element_type: Type::FUNCREF,
            },
            kind: crate::ir::module::TableKind::Local,
        });

        // Define function: void test()
        let block = allocator.alloc(Expression {
            kind: ExpressionKind::Block {
                name: None,
                list: BumpVec::new_in(&allocator),
            },
            type_: Type::NONE,
        });

        let func = Function::new(
            "test_func".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(ExprRef::new(block)),
        );
        let func_id = module.add_function(func);

        // Add to table (Elements)
        let offset = Expression::const_expr(&allocator, crate::ir::Literal::I32(0), Type::I32);
        let segment = ElementSegment {
            table_index: TableId::from_raw_parts(0, 0),
            offset,
            func_indices: vec![func_id],
        };
        module.add_element_segment(segment);

        let mut pass = GenerateDynCalls;
        pass.run(&mut module);

        // Verify export
        let export = module.exports.iter().find(|e| e.name == "dynCall_v");
        assert!(export.is_some());
    }

    #[test]
    fn test_generate_dyncalls_i32_void() {
        let allocator = Bump::new();
        let mut module = Module::new(&allocator);

        // Add a table
        module.add_table(crate::ir::module::Table {
            name: "table".to_string(),
            limits: crate::ir::module::TableLimits {
                initial: 1,
                maximum: None,
                element_type: Type::FUNCREF,
            },
            kind: crate::ir::module::TableKind::Local,
        });

        // Define function: i32 test()
        let val = Expression::const_expr(&allocator, crate::ir::Literal::I32(42), Type::I32);
        let func = Function::new(
            "test_func_i".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(val),
        );
        let func_id = module.add_function(func);

        // Add to table
        let offset = Expression::const_expr(&allocator, crate::ir::Literal::I32(0), Type::I32);
        let segment = ElementSegment {
            table_index: TableId::from_raw_parts(0, 0),
            offset,
            func_indices: vec![func_id],
        };
        module.add_element_segment(segment);

        let mut pass = GenerateDynCalls;
        pass.run(&mut module);

        // Expect: dynCall_i
        assert!(module.functions.iter().any(|(_, f)| f.name == "dynCall_i"));
    }
}
