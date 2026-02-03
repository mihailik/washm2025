use crate::collections::id_arena::IdArena;
use crate::ir::expression::{
    Expression, ExpressionId, ExpressionKind, ExpressionMarker, IrBuilder,
};
use crate::ir::module::{Function, Module};
use crate::ir::visitor::Visitor;
use crate::ir::{Literal, LocalId, Type};
use crate::passes::Pass;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

/// ConstHoisting pass: Move repeated constants to locals to reduce code size
///
/// 1. Count occurrences of each constant
/// 2. If constant used > N times, introduce local
/// 3. Replace all uses with local.get
pub struct ConstHoisting;

impl Pass for ConstHoisting {
    fn name(&self) -> &str {
        "const-hoisting"
    }

    fn run(&mut self, module: &mut Module) {
        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        for (_, func) in functions {
            Self::process_function(func, arena);
        }
    }
}

/// Wrapper for Literal to support Hash/Eq for use in HashMap
#[derive(Debug, Clone)]
struct HashableLiteral(Literal);

impl PartialEq for HashableLiteral {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for HashableLiteral {}

impl Hash for HashableLiteral {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match &self.0 {
            Literal::I32(val) => {
                0u8.hash(state);
                val.hash(state);
            }
            Literal::I64(val) => {
                1u8.hash(state);
                val.hash(state);
            }
            Literal::F32(val) => {
                2u8.hash(state);
                val.to_bits().hash(state);
            }
            Literal::F64(val) => {
                3u8.hash(state);
                val.to_bits().hash(state);
            }
            Literal::V128(val) => {
                4u8.hash(state);
                val.hash(state);
            }
        }
    }
}

impl ConstHoisting {
    fn process_function(func: &mut Function, arena: &mut IdArena<ExpressionMarker, Expression>) {
        let mut lit_to_local = HashMap::new();
        let mut candidates = Vec::new();

        let counts = if let Some(body_id) = func.body_mut() {
            // 1. Scan
            let mut scanner = ConstScanner::new();
            scanner.visit(body_id, arena);
            scanner.counts
        } else {
            return;
        };

        // 2. Identify candidates and add locals
        for (lit, count) in counts {
            if count >= 2 {
                let ty = match lit.0 {
                    Literal::I32(_) => Type::I32,
                    Literal::I64(_) => Type::I64,
                    Literal::F32(_) => Type::F32,
                    Literal::F64(_) => Type::F64,
                    _ => continue,
                };

                // Add new var to function and get its stable ID
                let new_id = func.add_var(ty);
                lit_to_local.insert(lit.clone(), new_id);
                candidates.push((lit.0, ty, new_id));
            }
        }

        if candidates.is_empty() {
            return;
        }

        if let Some(body_id) = func.body_mut() {
            // 3. Rewrite body
            {
                let mut rewriter = ConstRewriter {
                    lit_to_local: &lit_to_local,
                };
                rewriter.visit(body_id, arena);
            }

            // 4. Prepend initialization
            let mut builder = IrBuilder::new(arena);
            let mut new_list = Vec::new();
            for (lit, _, local_id) in candidates {
                let const_expr = builder.const_(lit);
                let set_expr = builder.local_set(local_id, const_expr);
                new_list.push(set_expr);
            }

            let body = *body_id;
            new_list.push(body);

            let old_ty = builder.arena[body].type_;
            let new_block = builder.block("", new_list, old_ty);
            *body_id = new_block;
        }
    }
}

struct ConstScanner {
    counts: HashMap<HashableLiteral, usize>,
}

impl ConstScanner {
    fn new() -> Self {
        Self {
            counts: HashMap::new(),
        }
    }
}

impl Visitor for ConstScanner {
    fn visit_expression(
        &mut self,
        expr_id: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        if let ExpressionKind::Const(lit) = &arena[*expr_id].kind {
            *self.counts.entry(HashableLiteral(*lit)).or_insert(0) += 1;
        }
        self.visit_children(expr_id, arena);
    }
}

struct ConstRewriter<'a> {
    lit_to_local: &'a HashMap<HashableLiteral, LocalId>,
}

impl<'a> Visitor for ConstRewriter<'a> {
    fn visit_expression(
        &mut self,
        expr_id: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        let mut replacement = None;
        if let ExpressionKind::Const(lit) = &arena[*expr_id].kind {
            if let Some(&local_id) = self.lit_to_local.get(&HashableLiteral(*lit)) {
                let lit = *lit;
                let ty = match lit {
                    Literal::I32(_) => Type::I32,
                    Literal::I64(_) => Type::I64,
                    Literal::F32(_) => Type::F32,
                    Literal::F64(_) => Type::F64,
                    _ => Type::NONE,
                };
                replacement = Some((local_id, ty));
            }
        }

        if let Some((local_id, ty)) = replacement {
            let mut builder = IrBuilder::new(arena);
            *expr_id = builder.local_get(local_id, ty);
            return; // Replaced, so no children to visit
        }
        self.visit_children(expr_id, arena);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::ops::BinaryOp;

    #[test]
    fn test_const_hoisting_basic() {
        let mut module = Module::new();
        let (expressions, _, _, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        let mut builder = IrBuilder::new(expressions);

        // (i32.add (i32.const 42) (i32.const 42))
        let c1 = builder.const_(Literal::I32(42));
        let c2 = builder.const_(Literal::I32(42));
        let add = builder.binary(BinaryOp::AddInt32, c1, c2, Type::I32);

        let mut func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![], // No params
            add,
        );

        // Manually run process_function logic since Pass::run needs Module
        ConstHoisting::process_function(&mut func, expressions);

        let body_id = func.body().unwrap();
        let body = &expressions[body_id];

        // Expecting:
        // (block
        //   (local.set $0 (i32.const 42))
        //   (i32.add (local.get $0) (local.get $0))
        // )

        match &body.kind {
            ExpressionKind::Block { list, .. } => {
                assert_eq!(list.len(), 2);
                // Check initialization
                let init = &expressions[list[0]];
                if let ExpressionKind::LocalSet { id, value } = &init.kind {
                    assert_eq!(id.index(), 0);
                    let val_expr = &expressions[*value];
                    assert!(matches!(
                        val_expr.kind,
                        ExpressionKind::Const(Literal::I32(42))
                    ));
                } else {
                    panic!("Expected LocalSet");
                }

                // Check body replacement
                let body_inner = &expressions[list[1]];
                if let ExpressionKind::Binary { left, right, .. } = &body_inner.kind {
                    let l_expr = &expressions[*left];
                    if let ExpressionKind::LocalGet { id, .. } = &l_expr.kind {
                        assert_eq!(id.index(), 0);
                    } else {
                        panic!("Expected LocalGet for left");
                    }
                    let r_expr = &expressions[*right];
                    if let ExpressionKind::LocalGet { id, .. } = &r_expr.kind {
                        assert_eq!(id.index(), 0);
                    } else {
                        panic!("Expected LocalGet for right");
                    }
                } else {
                    panic!("Expected Binary Add");
                }

                assert_eq!(func.locals().len(), 1); // Added 1 local
            }
            _ => panic!("Expected Block wrapper"),
        }
    }
}
