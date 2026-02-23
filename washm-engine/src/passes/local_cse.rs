use crate::collections::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::module::Module;
use crate::ir::module::{Local, LocalMarker};
use crate::ir::visitor::Visitor;
use crate::passes::Pass;
use std::collections::HashMap;

/// Local Common Subexpression Elimination (CSE)
///
/// Eliminates redundant computations within a function by detecting
/// expressions that compute the same value and replacing duplicates
/// with references to a single computation.
///
/// This pass is "local" because it works within function boundaries
/// and doesn't require global analysis.
pub struct LocalCSE;

impl Pass for LocalCSE {
    fn name(&self) -> &str {
        "local-cse"
    }

    fn run(&mut self, module: &mut Module) {
        for (_, func) in &mut module.functions {
            match &mut func.kind {
                crate::ir::module::FunctionKind::Local { body, locals, .. } => {
                    let mut cse = CSETransformer {
                        expr_map: HashMap::new(),
                        locals,
                    };
                    cse.visit(body, &mut module.expressions);
                }
                crate::ir::module::FunctionKind::Import(_) => {}
            }
        }
    }
}

struct CSETransformer<'a> {
    expr_map: HashMap<ExprKey, (ExpressionId, crate::ir::LocalId)>, // expr -> (original, temp_local)
    locals: &'a mut IdArena<LocalMarker, Local>,
}

/// Key for identifying equivalent expressions
#[derive(Hash, Eq, PartialEq, Clone, Debug)]
enum ExprKey {
    Binary {
        op: String,
        left: Box<ExprKey>,
        right: Box<ExprKey>,
    },
    Unary {
        op: String,
        value: Box<ExprKey>,
    },
    Const {
        value: String,
    },
    LocalGet {
        id: crate::ir::LocalId,
    },
}

impl<'a> CSETransformer<'a> {
    fn expr_to_key(
        &self,
        expr_id: ExpressionId,
        arena: &IdArena<ExpressionMarker, Expression>,
    ) -> Option<ExprKey> {
        let expr = &arena[expr_id];
        match &expr.kind {
            ExpressionKind::Const(lit) => Some(ExprKey::Const {
                value: format!("{:?}", lit),
            }),
            ExpressionKind::LocalGet { id } => Some(ExprKey::LocalGet { id: *id }),
            ExpressionKind::Binary { op, left, right } => {
                let left_key = self.expr_to_key(*left, arena)?;
                let right_key = self.expr_to_key(*right, arena)?;
                Some(ExprKey::Binary {
                    op: format!("{:?}", op),
                    left: Box::new(left_key),
                    right: Box::new(right_key),
                })
            }
            ExpressionKind::Unary { op, value } => {
                let value_key = self.expr_to_key(*value, arena)?;
                Some(ExprKey::Unary {
                    op: format!("{:?}", op),
                    value: Box::new(value_key),
                })
            }
            _ => None, // Only handle simple cases for now
        }
    }
}

impl<'a> Visitor for CSETransformer<'a> {
    fn visit(
        &mut self,
        expr: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        // Bottom-up: visit children first
        self.visit_children(expr, arena);
        self.visit_expression(expr, arena);
    }

    fn visit_expression(
        &mut self,
        expr: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        // Only consider expressions that don't have side effects for CSE
        if let Some(key) = self.expr_to_key(*expr, arena) {
            if let Some((_, temp_local_id)) = self.expr_map.get(&key) {
                // Common subexpression found: replace current expr with local.get
                let type_ = arena[*expr].type_;
                *expr = Expression::alloc(
                    arena,
                    ExpressionKind::LocalGet { id: *temp_local_id },
                    type_,
                );
            } else {
                // New common subexpression: store in a local.tee and add to map
                let type_ = arena[*expr].type_;
                let new_local_id = self.locals.alloc(crate::ir::module::Local {
                    name: String::new(),
                    type_,
                    h_type: None,
                });

                // Create a local.tee to store the result of the current expression
                let original_id = *expr;
                let tee_id = Expression::alloc(
                    arena,
                    ExpressionKind::LocalTee {
                        id: new_local_id,
                        value: original_id,
                    },
                    type_,
                );

                // Store the key and its temp local index in the map
                self.expr_map.insert(key, (tee_id, new_local_id));

                // Replace the current expr with the local.tee
                *expr = tee_id;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::{Expression, ExpressionKind};
    use crate::ir::module::Function;
    use crate::ir::ops::BinaryOp;
    use crate::ir::{Literal, Type};

    #[test]
    fn test_local_cse_basic() {
        let mut module = Module::new();

        // Create: (i32.add (i32.const 1) (i32.const 2))
        let const1 = module.expressions.alloc(Expression::new(
            ExpressionKind::Const(Literal::I32(1)),
            Type::I32,
        ));
        let const2 = module.expressions.alloc(Expression::new(
            ExpressionKind::Const(Literal::I32(2)),
            Type::I32,
        ));
        let add = module.expressions.alloc(Expression::new(
            ExpressionKind::Binary {
                op: BinaryOp::AddInt32,
                left: const1,
                right: const2,
            },
            Type::I32,
        ));

        let func = Function::new("test".to_string(), Type::NONE, Type::I32, vec![], add);

        // We need to add the function and get a mutable reference to it
        let func_id = module.add_function(func);

        let mut body = module.functions[func_id].body().unwrap();
        {
            let func_mut = &mut module.functions[func_id];
            if let crate::ir::module::FunctionKind::Local { locals, .. } = &mut func_mut.kind {
                let mut cse = CSETransformer {
                    expr_map: HashMap::new(),
                    locals,
                };

                cse.visit(&mut body, &mut module.expressions);
            }
        }

        if let Some(mut_body) = module.functions[func_id].body_mut() {
            *mut_body = body;
        }

        // For now, expression should be unchanged (foundation only)
        let body = module.functions[func_id].body().unwrap();
        assert!(matches!(
            module.expressions[body].kind,
            ExpressionKind::Binary { .. }
        ));
    }

    #[test]
    fn test_local_cse_can_generate_keys() {
        let mut module = Module::new();

        let add_body = module.expressions.alloc(Expression::new(
            ExpressionKind::Const(Literal::I32(0)),
            Type::I32,
        ));
        let mut func = Function::new("test".to_string(), Type::NONE, Type::I32, vec![], add_body);

        let transformer = CSETransformer {
            expr_map: HashMap::new(),
            locals: func.locals_mut(),
        };

        // Test constant key generation
        let const_expr = module.expressions.alloc(Expression::new(
            ExpressionKind::Const(Literal::I32(42)),
            Type::I32,
        ));
        let key = transformer.expr_to_key(const_expr, &module.expressions);
        assert!(key.is_some());

        // Test local.get key generation
        let get_expr = module.expressions.alloc(Expression::new(
            ExpressionKind::LocalGet {
                id: crate::ir::LocalId::from_u32(0),
            },
            Type::I32,
        ));
        let key = transformer.expr_to_key(get_expr, &module.expressions);
        assert!(key.is_some());
    }

    #[test]
    fn test_local_cse_preserves_structure() {
        let mut module = Module::new();

        // Complex expression that CSE doesn't currently optimize
        let val1 = module.expressions.alloc(Expression::new(
            ExpressionKind::Const(Literal::I32(10)),
            Type::I32,
        ));
        let val2 = module.expressions.alloc(Expression::new(
            ExpressionKind::Const(Literal::I32(20)),
            Type::I32,
        ));
        let add = module.expressions.alloc(Expression::new(
            ExpressionKind::Binary {
                op: BinaryOp::AddInt32,
                left: val1,
                right: val2,
            },
            Type::I32,
        ));

        let func = Function::new("test".to_string(), Type::NONE, Type::I32, vec![], add);

        let func_id = module.add_function(func);

        let mut pass = LocalCSE;
        pass.run(&mut module);

        // Should preserve structure
        let func = &module.functions[func_id];
        let body = func.body().unwrap();
        assert!(matches!(
            module.expressions[body].kind,
            ExpressionKind::Binary { .. }
        ));
    }
}
