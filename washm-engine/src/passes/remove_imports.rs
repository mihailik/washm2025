use crate::ir::expression::{ExprRef, Expression, ExpressionKind};
use crate::ir::module::{ImportKind, Module};
use crate::ir::visitor::Visitor;
use crate::ir::{Literal, Type};
use crate::passes::Pass;
use std::collections::HashSet;

pub struct RemoveImports;

impl Pass for RemoveImports {
    fn name(&self) -> &str {
        "RemoveImports"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        // 1. Identify imported functions
        let mut imported_functions = HashSet::new();
        let mut imported_sigs = std::collections::HashMap::new();

        for import in &module.imports {
            if let ImportKind::Function(_params, results) = import.kind {
                imported_functions.insert(import.name.clone());
                imported_sigs.insert(import.name.clone(), results);
            }
        }

        if imported_functions.is_empty() {
            return;
        }

        // 2. Identify functions used in tables (to preserve imports if they are indirect targets)
        let mut indirect_names = HashSet::new();
        for segment in &module.elements {
            for &idx in &segment.func_indices {
                if (idx as usize) < module.imports.len() {
                    if let Some(import) = module.imports.get(idx as usize) {
                        indirect_names.insert(import.name.clone());
                    }
                }
            }
        }

        // 3. Walk all function bodies and replace calls to imports
        for func in &mut module.functions {
            if let Some(body) = &mut func.body {
                let mut visitor = CallReplacer {
                    imported_functions: &imported_functions,
                    imported_sigs: &imported_sigs,
                    bump: module.allocator,
                };
                visitor.visit(body);
            }
        }

        // 4. Remove imports that are not in indirect_names and build remapping
        let mut func_remap = std::collections::HashMap::new();
        let mut current_func_idx = 0;
        let mut next_func_idx = 0;

        let old_imports = std::mem::take(&mut module.imports);
        for import in old_imports {
            if let ImportKind::Function(_, _) = import.kind {
                let keep = if imported_functions.contains(&import.name) {
                    indirect_names.contains(&import.name)
                } else {
                    true
                };

                if keep {
                    func_remap.insert(current_func_idx, next_func_idx);
                    next_func_idx += 1;
                    module.imports.push(import);
                }
                current_func_idx += 1;
            } else {
                module.imports.push(import);
            }
        }

        // Add defined functions to the remap
        for _ in 0..module.functions.len() {
            func_remap.insert(current_func_idx, next_func_idx);
            current_func_idx += 1;
            next_func_idx += 1;
        }

        // 5. Update references that use indices (Elements, Exports, Start)
        for segment in &mut module.elements {
            segment.func_indices.retain_mut(|idx| {
                if let Some(&new_idx) = func_remap.get(idx) {
                    *idx = new_idx;
                    true
                } else {
                    false
                }
            });
        }

        module.exports.retain_mut(|export| {
            if export.kind == crate::ir::module::ExportKind::Function {
                if let Some(&new_idx) = func_remap.get(&export.index) {
                    export.index = new_idx;
                    true
                } else {
                    false
                }
            } else {
                true
            }
        });

        if let Some(start_idx) = module.start {
            if let Some(&new_idx) = func_remap.get(&start_idx) {
                module.start = Some(new_idx);
            } else {
                module.start = None;
            }
        }
    }
}

struct CallReplacer<'a, 'b> {
    imported_functions: &'b HashSet<String>,
    imported_sigs: &'b std::collections::HashMap<String, Type>,
    bump: &'a bumpalo::Bump,
}

impl<'a, 'b> Visitor<'a> for CallReplacer<'a, 'b> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        self.visit_children(expr);

        if let ExpressionKind::Call { target, .. } = &expr.kind {
            if self.imported_functions.contains(*target) {
                // Found a call to an imported function
                let result_type = self
                    .imported_sigs
                    .get(*target)
                    .copied()
                    .unwrap_or(Type::NONE);

                // Replace with Nop or Const
                if result_type == Type::NONE {
                    // Replace with Nop
                    *expr = Expression::nop(self.bump);
                } else {
                    // Try to create a zero literal for the type
                    let literal = match result_type {
                        Type::I32 => Some(Literal::I32(0)),
                        Type::I64 => Some(Literal::I64(0)),
                        Type::F32 => Some(Literal::F32(0.0)),
                        Type::F64 => Some(Literal::F64(0.0)),
                        Type::V128 => Some(Literal::V128([0; 16])),
                        // For reference types, we don't have a Literal representation yet
                        // so we replace with Unreachable
                        _ => None,
                    };

                    if let Some(lit) = literal {
                        *expr = Expression::const_expr(self.bump, lit, result_type);
                    } else {
                        // Fallback for types we can't create constants for (e.g. references)
                        // Use Unreachable, which is valid in any context
                        *expr = Expression::new(
                            self.bump,
                            ExpressionKind::Unreachable,
                            Type::UNREACHABLE,
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::{ExpressionKind, IrBuilder};
    use crate::ir::module::{ElementSegment, Function, Import, ImportKind, Module};
    use crate::ir::{Literal, Type};
    use bumpalo::collections::Vec as BumpVec;
    use bumpalo::Bump;

    #[test]
    fn test_remove_imports_void_call() {
        let bump = Bump::new();
        let mut module = Module::new(&bump);

        // Add import
        module.add_import(Import {
            module: "env".to_string(),
            name: "imported_func".to_string(),
            kind: ImportKind::Function(Type::NONE, Type::NONE),
        });

        // Add caller function
        let builder = IrBuilder::new(&bump);
        let call = builder.call("imported_func", BumpVec::new_in(&bump), Type::NONE, false);

        let func = Function::new(
            "caller".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(call),
        );
        module.add_function(func);

        // Run pass
        let mut pass = RemoveImports;
        pass.run(&mut module);

        // Check if import is gone
        assert!(module.imports.is_empty(), "Import should be removed");

        // Check if call is replaced by Nop
        let body = module.functions[0].body.as_ref().unwrap();
        assert!(
            matches!(body.kind, ExpressionKind::Nop),
            "Call should be replaced by Nop"
        );
    }

    #[test]
    fn test_remove_imports_value_call() {
        let bump = Bump::new();
        let mut module = Module::new(&bump);

        // Add import returning i32
        module.add_import(Import {
            module: "env".to_string(),
            name: "imported_func_i32".to_string(),
            kind: ImportKind::Function(Type::NONE, Type::I32),
        });

        // Add caller function
        let builder = IrBuilder::new(&bump);
        let call = builder.call(
            "imported_func_i32",
            BumpVec::new_in(&bump),
            Type::I32,
            false,
        );
        let drop = builder.drop(call); // We drop it to form a valid block if needed, but here it's the body

        let func = Function::new(
            "caller".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(drop), // The body is a drop of the call
        );
        module.add_function(func);

        // Run pass
        let mut pass = RemoveImports;
        pass.run(&mut module);

        // Check if import is gone
        assert!(module.imports.is_empty(), "Import should be removed");

        // Check if call is replaced by Const
        let body = module.functions[0].body.as_ref().unwrap();
        // Body is Drop(Call) -> Drop(Const)
        if let ExpressionKind::Drop { value } = &body.kind {
            assert!(
                matches!(value.kind, ExpressionKind::Const(Literal::I32(0))),
                "Call should be replaced by Const(0)"
            );
        } else {
            panic!("Expected Drop");
        }
    }

    #[test]
    fn test_remove_imports_preserved_in_table() {
        let bump = Bump::new();
        let mut module = Module::new(&bump);

        // Add import
        module.add_import(Import {
            module: "env".to_string(),
            name: "imported_func".to_string(),
            kind: ImportKind::Function(Type::NONE, Type::NONE),
        });

        // Add to table
        // Index 0 matches the first import
        module.add_element_segment(ElementSegment {
            table_index: 0,
            offset: Expression::const_expr(&bump, Literal::I32(0), Type::I32),
            func_indices: vec![0],
        });

        // Run pass
        let mut pass = RemoveImports;
        pass.run(&mut module);

        // Check if import is preserved
        assert_eq!(
            module.imports.len(),
            1,
            "Import should be preserved because it is in table"
        );
    }
}
