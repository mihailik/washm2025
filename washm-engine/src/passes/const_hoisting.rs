use crate::ir::expression::{ExprRef, ExpressionKind, IrBuilder};
use crate::ir::module::Module;
use crate::ir::visitor::Visitor;
use crate::ir::{Literal, Type};
use crate::passes::Pass;
use bumpalo::collections::Vec as BumpVec;
use std::collections::HashMap;

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

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        let allocator = module.allocator;
        for (_, func) in &mut module.functions {
            Self::process_function(func, allocator);
        }
    }
}

use std::hash::{Hash, Hasher};

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
    fn process_function<'a>(
        func: &mut crate::ir::module::Function<'a>,
        allocator: &'a bumpalo::Bump,
    ) {
        let mut lit_to_local = HashMap::new();
        let mut candidates = Vec::new();

        let counts = if let Some(body) = func.body_mut() {
            // 1. Scan
            let mut scanner = ConstScanner::new();
            scanner.visit(body);
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

        if let Some(body) = func.body_mut() {
            // 3. Rewrite body
            let builder = IrBuilder::new(allocator);
            let mut rewriter = ConstRewriter {
                lit_to_local: &lit_to_local,
                builder,
                replaced_count: 0,
            };
            rewriter.visit(body);

            // 4. Prepend initialization
            let mut new_list = BumpVec::new_in(allocator);
            for (lit, _, local_id) in candidates {
                let const_expr = builder.const_(lit);
                let set_expr = builder.local_set(local_id, const_expr);
                new_list.push(set_expr);
            }

            new_list.push(*body);

            let new_block = builder.block(None, new_list, body.type_);
            *body = new_block;
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

impl<'a> Visitor<'a> for ConstScanner {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        if let ExpressionKind::Const(lit) = &expr.kind {
            *self.counts.entry(HashableLiteral(*lit)).or_insert(0) += 1;
        }
        self.visit_children(expr);
    }
}

struct ConstRewriter<'a, 'b> {
    lit_to_local: &'b HashMap<HashableLiteral, crate::ir::LocalId>,
    builder: IrBuilder<'a>,
    replaced_count: usize,
}

impl<'a, 'b> Visitor<'a> for ConstRewriter<'a, 'b> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        if let ExpressionKind::Const(lit) = &expr.kind {
            if let Some(&local_id) = self.lit_to_local.get(&HashableLiteral(*lit)) {
                // Replace with local.get
                // Type should match literal type
                let ty = match lit {
                    Literal::I32(_) => Type::I32,
                    Literal::I64(_) => Type::I64,
                    Literal::F32(_) => Type::F32,
                    Literal::F64(_) => Type::F64,
                    _ => Type::NONE,
                };
                *expr = self.builder.local_get(local_id, ty);
                self.replaced_count += 1;
            }
        }
        self.visit_children(expr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::ExpressionKind;
    use crate::ir::module::Function;
    use crate::ir::ops::BinaryOp;
    use bumpalo::Bump;

    #[test]
    fn test_const_hoisting_basic() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // (i32.add (i32.const 42) (i32.const 42))
        let c1 = builder.const_(Literal::I32(42));
        let c2 = builder.const_(Literal::I32(42));
        let add = builder.binary(BinaryOp::AddInt32, c1, c2, Type::I32);

        let mut func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![], // No params
            Some(add),
        );

        // Manually run process_function logic since Pass::run needs Module
        ConstHoisting::process_function(&mut func, &bump);

        let body = func.body().unwrap();

        // Expecting:
        // (block
        //   (local.set $0 (i32.const 42))
        //   (i32.add (local.get $0) (local.get $0))
        // )

        match &body.kind {
            ExpressionKind::Block { list, .. } => {
                assert_eq!(list.len(), 2);
                // Check initialization
                if let ExpressionKind::LocalSet { id, value } = &list[0].kind {
                    assert_eq!(id.index(), 0);
                    assert!(matches!(
                        value.kind,
                        ExpressionKind::Const(Literal::I32(42))
                    ));
                } else {
                    panic!("Expected LocalSet");
                }

                // Check body replacement
                if let ExpressionKind::Binary { left, right, .. } = &list[1].kind {
                    if let ExpressionKind::LocalGet { id, .. } = &left.kind {
                        assert_eq!(id.index(), 0);
                    } else {
                        panic!("Expected LocalGet for left");
                    }
                    if let ExpressionKind::LocalGet { id, .. } = &right.kind {
                        assert_eq!(id.index(), 0);
                    } else {
                        panic!("Expected LocalGet for right");
                    }
                } else {
                    panic!("Expected Binary Add");
                }

                assert_eq!(func.locals.len() - func.num_params(), 1); // Added 1 local
            }
            _ => panic!("Expected Block wrapper"),
        }
    }
}
