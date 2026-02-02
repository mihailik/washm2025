use crate::collections::IdArena;
use crate::ir::expression::{ExprRef, Expression, ExpressionKind};
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

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        let allocator = module.allocator();

        for (_, func) in &mut module.functions {
            match &mut func.kind {
                crate::ir::module::FunctionKind::Local { body, locals, .. } => {
                    let mut cse = CSETransformer {
                        allocator,
                        expr_map: HashMap::new(),
                        locals,
                    };
                    cse.visit(body);
                }
                crate::ir::module::FunctionKind::Import(_) => {}
            }
        }
    }
}

#[allow(dead_code)]
struct CSETransformer<'a, 'b> {
    allocator: &'a bumpalo::Bump,
    expr_map: HashMap<ExprKey, (ExprRef<'a>, crate::ir::LocalId)>, // expr -> (original, temp_local)
    locals: &'b mut IdArena<LocalMarker, Local>,
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

impl<'a, 'b> CSETransformer<'a, 'b> {
    fn expr_to_key(&self, expr: &ExprRef<'a>) -> Option<ExprKey> {
        match &expr.kind {
            ExpressionKind::Const(lit) => Some(ExprKey::Const {
                value: format!("{:?}", lit),
            }),
            ExpressionKind::LocalGet { id } => Some(ExprKey::LocalGet { id: *id }),
            ExpressionKind::Binary { op, left, right } => {
                let left_key = self.expr_to_key(left)?;
                let right_key = self.expr_to_key(right)?;
                Some(ExprKey::Binary {
                    op: format!("{:?}", op),
                    left: Box::new(left_key),
                    right: Box::new(right_key),
                })
            }
            ExpressionKind::Unary { op, value } => {
                let value_key = self.expr_to_key(value)?;
                Some(ExprKey::Unary {
                    op: format!("{:?}", op),
                    value: Box::new(value_key),
                })
            }
            _ => None, // Only handle simple cases for now
        }
    }
}

impl<'a, 'b> Visitor<'a> for CSETransformer<'a, 'b> {
    fn visit(&mut self, expr: &mut ExprRef<'a>) {
        // Bottom-up: visit children first
        self.visit_children(expr);
        self.visit_expression(expr);
    }

    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        if let Some(expr_mut) = unsafe { expr.as_ptr().as_mut() } {
            // Only consider expressions that don't have side effects for CSE
            if let Some(key) = self.expr_to_key(expr) {
                if let Some((_, temp_local_id)) = self.expr_map.get(&key) {
                    // Common subexpression found: replace current expr with local.get
                    *expr_mut = Expression {
                        type_: expr_mut.type_, // Keep the original type of the expression
                        kind: ExpressionKind::LocalGet { id: *temp_local_id },
                    };
                } else {
                    // New common subexpression: store in a local.tee and add to map
                    let new_local_id = self.locals.alloc(crate::ir::module::Local {
                        name: String::new(),
                        type_: expr_mut.type_,
                    });

                    // Create a local.tee to store the result of the current expression
                    let tee_expr =
                        Expression::local_tee(self.allocator, new_local_id, *expr, expr_mut.type_);

                    // Store the key and its temp local index in the map
                    self.expr_map.insert(key, (tee_expr, new_local_id)); // Store the tee_expr as the original

                    // Replace the current expr with the local.tee
                    *expr = tee_expr;
                }
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
    use bumpalo::Bump;

    #[test]
    fn test_local_cse_basic() {
        let bump = Bump::new();

        // Create: (i32.add (i32.const 1) (i32.const 2))
        let const1 = Expression::const_expr(&bump, Literal::I32(1), Type::I32);
        let const2 = Expression::const_expr(&bump, Literal::I32(2), Type::I32);
        let add = Expression::new(
            &bump,
            ExpressionKind::Binary {
                op: BinaryOp::AddInt32,
                left: const1,
                right: const2,
            },
            Type::I32,
        );

        let func = Function::new("test".to_string(), Type::NONE, Type::I32, vec![], Some(add));

        let mut module = Module::new(&bump);
        // We need to add the function and get a mutable reference to it
        let func_id = module.add_function(func);

        let mut body = module.functions[func_id].body().unwrap();
        {
            let func_mut = &mut module.functions[func_id];
            let mut cse = CSETransformer {
                allocator: &bump,
                expr_map: HashMap::new(),
                locals: func_mut.locals_mut(),
            };

            cse.visit(&mut body);
        }

        if let Some(mut_body) = module.functions[func_id].body_mut() {
            *mut_body = body;
        }

        // For now, expression should be unchanged (foundation only)
        let body = module.functions[func_id].body().unwrap();
        assert!(matches!(body.kind, ExpressionKind::Binary { .. }));
    }

    #[test]
    fn test_local_cse_can_generate_keys() {
        let bump = Bump::new();

        let mut func = Function::new("test".to_string(), Type::NONE, Type::I32, vec![], None);

        let transformer = CSETransformer {
            allocator: &bump,
            expr_map: HashMap::new(),
            locals: func.locals_mut(),
        };

        // Test constant key generation
        let const_expr = Expression::const_expr(&bump, Literal::I32(42), Type::I32);
        let key = transformer.expr_to_key(&const_expr);
        assert!(key.is_some());

        // Test local.get key generation
        let get_expr = Expression::local_get(&bump, crate::ir::LocalId::from_u32(0), Type::I32);
        let key = transformer.expr_to_key(&get_expr);
        assert!(key.is_some());
    }

    #[test]
    fn test_local_cse_preserves_structure() {
        let bump = Bump::new();

        // Complex expression that CSE doesn't currently optimize
        let val1 = Expression::const_expr(&bump, Literal::I32(10), Type::I32);
        let val2 = Expression::const_expr(&bump, Literal::I32(20), Type::I32);
        let add = Expression::new(
            &bump,
            ExpressionKind::Binary {
                op: BinaryOp::AddInt32,
                left: val1,
                right: val2,
            },
            Type::I32,
        );

        let func = Function::new("test".to_string(), Type::NONE, Type::I32, vec![], Some(add));

        let mut module = Module::new(&bump);
        let func_id = module.add_function(func);

        let mut pass = LocalCSE;
        pass.run(&mut module);

        // Should preserve structure
        let func = &module.functions[func_id];
        let body = func.body().unwrap();
        assert!(matches!(body.kind, ExpressionKind::Binary { .. }));
    }
}
