use crate::ir::expression::{ExprRef, Expression, ExpressionKind};
use crate::ir::module::Module;
use crate::ir::visitor::Visitor;
use crate::ir::{FuncId, Literal, Type};
use crate::passes::Pass;
use std::collections::{HashMap, HashSet};

pub struct RemoveImports;

impl Pass for RemoveImports {
    fn name(&self) -> &str {
        "RemoveImports"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        // 1. Identify imported functions that can be removed
        let mut to_replace = HashSet::new();
        let mut imported_sigs = HashMap::new();
        let mut preserved = HashSet::new();

        for (_, segment) in &module.elements {
            for &id in &segment.func_indices {
                preserved.insert(id);
            }
        }
        for export in &module.exports {
            if let crate::ir::module::ExportKind::Function(id) = export.kind {
                preserved.insert(id);
            }
        }
        if let Some(id) = module.start {
            preserved.insert(id);
        }

        for (id, func) in &module.functions {
            if func.is_import() && !preserved.contains(&id) {
                to_replace.insert(id);
                imported_sigs.insert(id, func.results);
            }
        }

        if to_replace.is_empty() {
            return;
        }

        // 3. Walk all function bodies and replace calls to imports
        for (_, func) in &mut module.functions {
            if let Some(body) = func.body_mut() {
                let mut visitor = CallReplacer {
                    to_replace: &to_replace,
                    imported_sigs: &imported_sigs,
                    bump: module.allocator,
                };
                visitor.visit(body);
            }
        }

        // 4. Remove the imports from the module
        let to_remove_ids = to_replace.clone();

        module
            .functions
            .retain(|id, _| !to_remove_ids.contains(&id));
    }
}

struct CallReplacer<'a, 'b> {
    to_replace: &'b HashSet<FuncId>,
    imported_sigs: &'b HashMap<FuncId, Type>,
    bump: &'a bumpalo::Bump,
}

impl<'a, 'b> Visitor<'a> for CallReplacer<'a, 'b> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        self.visit_children(expr);

        if let ExpressionKind::Call { target, .. } = &expr.kind {
            if self.to_replace.contains(target) {
                // Found a call to an imported function
                let result_type = self
                    .imported_sigs
                    .get(target)
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
        let imported_func_id = module.add_function(Function::new_import(
            "imported_func".to_string(),
            "env".to_string(),
            "imported_func".to_string(),
            Type::NONE,
            Type::NONE,
        ));

        // Add caller function
        let builder = IrBuilder::new(&bump);
        let call = builder.call(imported_func_id, BumpVec::new_in(&bump), Type::NONE, false);

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
        assert!(
            module.functions.iter().all(|(_, f)| !f.is_import()),
            "Import should be removed"
        );

        // Check if call is replaced by Nop
        let binding = module.functions[module.functions.iter().next().unwrap().0].body();
        let body = binding.as_ref().unwrap();
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
        let imported_func_id = module.functions.iter().last().unwrap().0;

        // Add caller function
        let builder = IrBuilder::new(&bump);
        let call = builder.call(imported_func_id, BumpVec::new_in(&bump), Type::I32, false);
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
        assert!(
            module.functions.iter().all(|(_, f)| !f.is_import()),
            "Import should be removed"
        );

        // Check if call is replaced by Const
        let binding = module.functions[module.functions.iter().next().unwrap().0].body();
        let body = binding.as_ref().unwrap();
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
        let f_id = module.add_function(Function::new_import(
            "imported_func".to_string(),
            "env".to_string(),
            "imported_func".to_string(),
            Type::NONE,
            Type::NONE,
        ));

        let table_id = module.add_table(crate::ir::module::Table {
            name: "table".to_string(),
            limits: crate::ir::module::TableLimits {
                element_type: Type::FUNCREF,
                initial: 1,
                maximum: None,
            },
            kind: crate::ir::module::TableKind::Local,
        });

        // Add to table
        module.add_element_segment(ElementSegment {
            table_index: table_id,
            offset: Expression::const_expr(&bump, Literal::I32(0), Type::I32),
            func_indices: vec![f_id],
        });

        // Run pass
        let mut pass = RemoveImports;
        pass.run(&mut module);

        // Check if import is preserved
        assert_eq!(
            module
                .functions
                .iter()
                .filter(|(_, f)| f.is_import())
                .count(),
            1,
            "Import should be preserved because it is in table"
        );
    }
}
