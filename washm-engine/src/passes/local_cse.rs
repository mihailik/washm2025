use crate::ir::expression::{ExprRef, Expression, ExpressionKind};
use crate::ir::module::Module;
use crate::passes::Pass;
use crate::ir::visitor::Visitor;
use crate::ir::Type;
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

        for func in &mut module.functions {
            if let Some(body) = &mut func.body {
                let mut cse = CSETransformer {
                    allocator,
                    expr_map: HashMap::new(),
                    local_counter: func.vars.len() as u32,
                    new_locals_to_add: Vec::new(),
                };
                cse.visit(body);
                func.vars.extend(cse.new_locals_to_add); // Append new locals
            }
        }
    }
}

#[allow(dead_code)]
struct CSETransformer<'a> {
    allocator: &'a bumpalo::Bump,
    expr_map: HashMap<ExprKey, (ExprRef<'a>, u32)>, // expr -> (original, temp_local)
    local_counter: u32,
    new_locals_to_add: Vec<Type>, // New field to collect types of new locals
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
        index: u32,
    },
}

impl<'a> CSETransformer<'a> {
    fn expr_to_key(&self, expr: &ExprRef<'a>) -> Option<ExprKey> {
        match &expr.kind {
            ExpressionKind::Const(lit) => Some(ExprKey::Const {
                value: format!("{:?}", lit),
            }),
            ExpressionKind::LocalGet { index } => Some(ExprKey::LocalGet { index: *index }),
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

impl<'a> Visitor<'a> for CSETransformer<'a> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        // Manually visit children for recursive traversal based on ExpressionKind variants
        if let Some(expr_mut) = unsafe { expr.as_ptr().as_mut() } {
            match &mut expr_mut.kind {
                ExpressionKind::Block { list, .. } => {
                    for e in list.iter_mut() {
                        self.visit_expression(e);
                    }
                }
                ExpressionKind::If {
                    condition,
                    if_true,
                    if_false,
                } => {
                    self.visit_expression(condition);
                    self.visit_expression(if_true);
                    if let Some(f) = if_false {
                        self.visit_expression(f);
                    }
                }
                ExpressionKind::Loop { body, .. } => {
                    self.visit_expression(body);
                }
                ExpressionKind::Unary { value, .. } => {
                    self.visit_expression(value);
                }
                ExpressionKind::Binary { left, right, .. } => {
                    self.visit_expression(left);
                    self.visit_expression(right);
                }
                ExpressionKind::Call { operands, .. } => {
                    for e in operands.iter_mut() {
                        self.visit_expression(e);
                    }
                }
                ExpressionKind::LocalSet { value, .. } => {
                    self.visit_expression(value);
                }
                ExpressionKind::LocalTee { value, .. } => {
                    self.visit_expression(value);
                }
                ExpressionKind::GlobalSet { value, .. } => {
                    self.visit_expression(value);
                }
                ExpressionKind::Break {
                    condition, value, ..
                } => {
                    if let Some(c) = condition {
                        self.visit_expression(c);
                    }
                    if let Some(v) = value {
                        self.visit_expression(v);
                    }
                }
                ExpressionKind::Return { value } => {
                    if let Some(v) = value {
                        self.visit_expression(v);
                    }
                }
                ExpressionKind::Drop { value } => {
                    self.visit_expression(value);
                }
                ExpressionKind::Select {
                    condition,
                    if_true,
                    if_false,
                } => {
                    self.visit_expression(condition);
                    self.visit_expression(if_true);
                    self.visit_expression(if_false);
                }
                ExpressionKind::Load { ptr, .. } => {
                    self.visit_expression(ptr);
                }
                ExpressionKind::Store { ptr, value, .. } => {
                    self.visit_expression(ptr);
                    self.visit_expression(value);
                }
                ExpressionKind::Switch {
                    condition, value, ..
                } => {
                    self.visit_expression(condition);
                    if let Some(v) = value {
                        self.visit_expression(v);
                    }
                }
                ExpressionKind::CallIndirect {
                    target, operands, ..
                } => {
                    self.visit_expression(target);
                    for e in operands.iter_mut() {
                        self.visit_expression(e);
                    }
                }
                ExpressionKind::MemoryGrow { delta } => {
                    self.visit_expression(delta);
                }
                ExpressionKind::AtomicRMW { ptr, value, .. } => {
                    self.visit_expression(ptr);
                    self.visit_expression(value);
                }
                ExpressionKind::AtomicCmpxchg {
                    ptr,
                    expected,
                    replacement,
                    ..
                } => {
                    self.visit_expression(ptr);
                    self.visit_expression(expected);
                    self.visit_expression(replacement);
                }
                ExpressionKind::AtomicWait {
                    ptr,
                    expected,
                    timeout,
                    ..
                } => {
                    self.visit_expression(ptr);
                    self.visit_expression(expected);
                    self.visit_expression(timeout);
                }
                ExpressionKind::AtomicNotify { ptr, count } => {
                    self.visit_expression(ptr);
                    self.visit_expression(count);
                }
                ExpressionKind::TupleMake { operands } => {
                    for e in operands.iter_mut() {
                        self.visit_expression(e);
                    }
                }
                ExpressionKind::TupleExtract { tuple, .. } => {
                    self.visit_expression(tuple);
                }
                ExpressionKind::RefIsNull { value } => {
                    self.visit_expression(value);
                }
                ExpressionKind::RefEq { left, right } => {
                    self.visit_expression(left);
                    self.visit_expression(right);
                }
                ExpressionKind::RefAs { value, .. } => {
                    self.visit_expression(value);
                }
                ExpressionKind::TableGet { index, .. } => {
                    self.visit_expression(index);
                }
                ExpressionKind::TableSet { index, value, .. } => {
                    self.visit_expression(index);
                    self.visit_expression(value);
                }
                ExpressionKind::TableGrow { value, delta, .. } => {
                    self.visit_expression(value);
                    self.visit_expression(delta);
                }
                ExpressionKind::TableFill {
                    dest, value, size, ..
                } => {
                    self.visit_expression(dest);
                    self.visit_expression(value);
                    self.visit_expression(size);
                }
                ExpressionKind::TableCopy {
                    dest, src, size, ..
                } => {
                    self.visit_expression(dest);
                    self.visit_expression(src);
                    self.visit_expression(size);
                }
                ExpressionKind::TableInit {
                    dest, offset, size, ..
                } => {
                    self.visit_expression(dest);
                    self.visit_expression(offset);
                    self.visit_expression(size);
                }
                ExpressionKind::MemoryInit {
                    dest, offset, size, ..
                } => {
                    self.visit_expression(dest);
                    self.visit_expression(offset);
                    self.visit_expression(size);
                }
                ExpressionKind::MemoryCopy { dest, src, size } => {
                    self.visit_expression(dest);
                    self.visit_expression(src);
                    self.visit_expression(size);
                }
                ExpressionKind::MemoryFill { dest, value, size } => {
                    self.visit_expression(dest);
                    self.visit_expression(value);
                    self.visit_expression(size);
                }
                ExpressionKind::I31New { value } => {
                    self.visit_expression(value);
                }
                ExpressionKind::I31Get { i31, .. } => {
                    self.visit_expression(i31);
                }
                ExpressionKind::SIMDExtract { vec, .. } => {
                    self.visit_expression(vec);
                }
                ExpressionKind::SIMDReplace { vec, value, .. } => {
                    self.visit_expression(vec);
                    self.visit_expression(value);
                }
                ExpressionKind::SIMDShuffle { left, right, .. } => {
                    self.visit_expression(left);
                    self.visit_expression(right);
                }
                ExpressionKind::SIMDTernary { a, b, c, .. } => {
                    self.visit_expression(a);
                    self.visit_expression(b);
                    self.visit_expression(c);
                }
                ExpressionKind::SIMDShift { vec, shift, .. } => {
                    self.visit_expression(vec);
                    self.visit_expression(shift);
                }
                ExpressionKind::SIMDLoad { ptr, .. } => {
                    self.visit_expression(ptr);
                }
                ExpressionKind::SIMDLoadStoreLane { ptr, vec, .. } => {
                    self.visit_expression(ptr);
                    self.visit_expression(vec);
                }
                ExpressionKind::StructNew { operands, .. } => {
                    for e in operands.iter_mut() {
                        self.visit_expression(e);
                    }
                }
                ExpressionKind::StructGet { ptr, .. } => {
                    self.visit_expression(ptr);
                }
                ExpressionKind::StructSet { ptr, value, .. } => {
                    self.visit_expression(ptr);
                    self.visit_expression(value);
                }
                ExpressionKind::ArrayNew { size, init, .. } => {
                    self.visit_expression(size);
                    if let Some(i) = init {
                        self.visit_expression(i);
                    }
                }
                ExpressionKind::ArrayGet { ptr, index, .. } => {
                    self.visit_expression(ptr);
                    self.visit_expression(index);
                }
                ExpressionKind::ArraySet {
                    ptr, index, value, ..
                } => {
                    self.visit_expression(ptr);
                    self.visit_expression(index);
                    self.visit_expression(value);
                }
                ExpressionKind::ArrayLen { ptr } => {
                    self.visit_expression(ptr);
                }
                ExpressionKind::Try {
                    body, catch_bodies, ..
                } => {
                    self.visit_expression(body);
                    for e in catch_bodies.iter_mut() {
                        self.visit_expression(e);
                    }
                }
                ExpressionKind::Throw { operands, .. } => {
                    for e in operands.iter_mut() {
                        self.visit_expression(e);
                    }
                }
                ExpressionKind::RefTest { value, .. } => {
                    self.visit_expression(value);
                }
                ExpressionKind::RefCast { value, .. } => {
                    self.visit_expression(value);
                }
                ExpressionKind::BrOn { value, .. } => {
                    self.visit_expression(value);
                }
                // No children for these variants
                ExpressionKind::Const(_)
                | ExpressionKind::LocalGet { .. }
                | ExpressionKind::GlobalGet { .. }
                | ExpressionKind::Unreachable
                | ExpressionKind::Nop
                | ExpressionKind::AtomicFence
                | ExpressionKind::RefNull { .. }
                | ExpressionKind::RefFunc { .. }
                | ExpressionKind::TableSize { .. }
                | ExpressionKind::MemorySize
                | ExpressionKind::DataDrop { .. }
                | ExpressionKind::ElemDrop { .. }
                | ExpressionKind::Rethrow { .. }
                | ExpressionKind::Pop { .. } => {}
            }
        }
        // After visiting children, perform the CSE logic for the current expression
        if let Some(expr_mut) = unsafe { expr.as_ptr().as_mut() } {
            // Only consider expressions that don't have side effects for CSE
            // For now, assume ExpressionKind covered by expr_to_key are pure.
            // A more robust check would involve `Effect::has_side_effects(expr_mut.kind)`.
            // The `expr_to_key` method already implicitly filters out many non-pure expressions.
            if let Some(key) = self.expr_to_key(expr) {
                if let Some((_, temp_local_idx)) = self.expr_map.get(&key) {
                    // Common subexpression found: replace current expr with local.get
                    *expr_mut = Expression {
                        type_: expr_mut.type_, // Keep the original type of the expression
                        kind: ExpressionKind::LocalGet {
                            index: *temp_local_idx,
                        },
                    };
                } else {
                    // New common subexpression: store in a local.tee and add to map
                    let new_local_idx = self.local_counter;
                    self.local_counter += 1;

                    // Add the new local's type to new_locals_to_add
                    self.new_locals_to_add.push(expr_mut.type_);

                    // Create a local.tee to store the result of the current expression
                    let tee_expr =
                        Expression::local_tee(self.allocator, new_local_idx, *expr, expr_mut.type_);

                    // Store the key and its temp local index in the map
                    self.expr_map.insert(key, (tee_expr, new_local_idx)); // Store the tee_expr as the original

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
        module.add_function(func);

        let mut pass = LocalCSE;
        pass.run(&mut module);

        // For now, expression should be unchanged (foundation only)
        let body = module.functions[0].body.as_ref().unwrap();
        assert!(matches!(body.kind, ExpressionKind::Binary { .. }));
    }

    #[test]
    fn test_local_cse_can_generate_keys() {
        let bump = Bump::new();

        let transformer = CSETransformer {
            allocator: &bump,
            expr_map: HashMap::new(),
            local_counter: 0,
            new_locals_to_add: Vec::new(), // Initialize the new field
        };

        // Test constant key generation
        let const_expr = Expression::const_expr(&bump, Literal::I32(42), Type::I32);
        let key = transformer.expr_to_key(&const_expr);
        assert!(key.is_some());

        // Test local.get key generation
        let get_expr = Expression::local_get(&bump, 0, Type::I32);
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
        module.add_function(func);

        let mut pass = LocalCSE;
        pass.run(&mut module);

        // Should preserve structure
        let body = module.functions[0].body.as_ref().unwrap();
        assert!(matches!(body.kind, ExpressionKind::Binary { .. }));
    }
}
