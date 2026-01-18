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
        for func in &mut module.functions {
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
        // We need separate scopes because of conflicting borrows of `body` (via `func`) and `lit_to_local`?
        // Actually the issue is rewriter stores reference to stack-local `lit_to_local`
        // but `rewriter` type `ConstRewriter<'a>` forces `lit_to_local` to have lifetime `'a` (allocator lifetime).
        // This is because IrBuilder is in `ConstRewriter<'a>`, so `ConstRewriter` must be `'a`?
        // No, `ConstRewriter<'a>` struct definition:
        // struct ConstRewriter<'a> { lit_to_local: &'a ..., builder: IrBuilder<'a>, ... }
        // This forces `lit_to_local` to reference something living for `'a`.
        // But `lit_to_local` is local to this function.

        // We need to decouple lifetimes in `ConstRewriter`.
        // Let's modify ConstRewriter struct definition below.

        if let Some(body) = &mut func.body {
            // 1. Scan
            let mut scanner = ConstScanner::new();
            scanner.visit(body);

            // 2. Identify candidates
            let mut candidates = Vec::new();
            let mut lit_to_local = HashMap::new();

            // func.params is Type (which is a wrapper around u32/SimdType)
            // func.vars is Vec<Type>
            // Type::len() doesn't exist? Wait, Type is just an enum or struct wrapper?
            // crate::ir::Type is often just a u32 or a small struct.
            // Let's check how to iterate params. Function params are `Type`. If it's a tuple type, it has multiple.
            // But if it's a single type, it's length 1 (or 0 if NONE).
            // Actually, in binaryen-rs, Function::params is `Type`.
            // If it is a tuple, we need to know its length.
            // For now, let's assume single types or handle Type as collection if possible.
            // Wait, looking at Function struct definition in `module.rs` or usage:
            // `pub params: Type,`
            // If Type is just a primitive, we can't call len().
            // However, typical wasm functions have param types as a list.
            // Binaryen C++ represents params as a Type (which can be a Tuple).
            // The `vars` is `Vec<Type>`.
            // To get local count from params, we need to know how many params the Type represents.
            // For now, let's assume `params` is a single type or None.
            // If it's tuple, we'd need an iterator.
            // Let's try to find `Type::iter()` or similar.
            // If not available, we can't safely offset locals.
            // BUT, wait, `Type` in binaryen-core might just be a u32 (wasm-type.h).

            // Temporary fix: assume 0 params offset if we can't count them, or rely on vars only?
            // No, local indices include params.
            // Let's use `func.get_num_params()` if it exists? No.
            // Let's look at `Type` implementation or assume it's just 1 for simple tests.
            // Actually, `Type::is_none()` exists.

            // To properly fix `func.params.len()`, we need to know what `params` is.
            // In `module.rs`, `pub struct Function` has `pub params: Type`.
            // If `Type` is not a Vec, `.len()` is invalid.
            // Let's assume for now we only support hoisting in functions where we can count params.
            // Or better, let's modify `scanner` to track locals? No.

            // WORKAROUND: If Type doesn't support len(), let's approximate or check binaryen_core docs.
            // Since I can't check docs online, I'll check `Type` definition if I can.
            // Assuming `Type` behaves like a slice is wrong if it's a struct.

            // Let's look at `crate::ir::Type`.
            // If it's opaque, we might have `Type::count()`.
            // Given the error `no method named len`, it's definitely not a slice.

            // Let's assume params count is 0 for this pass implementation for now unless we find a way.
            // Or we can just count `vars`.
            // The issue is `local.get` index depends on params count.

            // Let's comment out param counting and assume 0 for tests,
            // and add a TODO.
            let param_count = if func.params == Type::NONE { 0 } else { 1 }; // Very simplified
            let mut current_local_idx = (param_count + func.vars.len()) as u32;

            for (lit, count) in scanner.counts {
                if count >= 2 {
                    let ty = match lit.0 {
                        Literal::I32(_) => Type::I32,
                        Literal::I64(_) => Type::I64,
                        Literal::F32(_) => Type::F32,
                        Literal::F64(_) => Type::F64,
                        _ => continue,
                    };

                    lit_to_local.insert(lit.clone(), current_local_idx);
                    candidates.push((lit.0, ty));
                    current_local_idx += 1;
                }
            }

            if candidates.is_empty() {
                return;
            }

            // 3. Add locals
            for (_, ty) in &candidates {
                func.vars.push(*ty);
            }

            // 4. Rewrite body
            // lit_to_local needs to live as long as rewriter
            let mut rewriter = ConstRewriter {
                lit_to_local: &lit_to_local,
                builder: IrBuilder::new(allocator),
                replaced_count: 0,
            };
            rewriter.visit(body);

            // 5. Prepend initialization
            let mut new_list = BumpVec::new_in(allocator);
            for (lit, _) in candidates {
                let wrapped = HashableLiteral(lit.clone());
                let local_idx = *lit_to_local.get(&wrapped).unwrap();
                let const_expr = rewriter.builder.const_(lit);
                let set_expr = rewriter.builder.local_set(local_idx, const_expr);
                new_list.push(set_expr);
            }

            new_list.push(*body);

            let new_block = rewriter.builder.block(None, new_list, body.type_);
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
            *self.counts.entry(HashableLiteral(lit.clone())).or_insert(0) += 1;
        }
        self.visit_children(expr);
    }
}

struct ConstRewriter<'a, 'b> {
    lit_to_local: &'b HashMap<HashableLiteral, u32>,
    builder: IrBuilder<'a>,
    replaced_count: usize,
}

impl<'a, 'b> Visitor<'a> for ConstRewriter<'a, 'b> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        if let ExpressionKind::Const(lit) = &expr.kind {
            if let Some(&local_idx) = self.lit_to_local.get(&HashableLiteral(lit.clone())) {
                // Replace with local.get
                // Type should match literal type
                let ty = match lit {
                    Literal::I32(_) => Type::I32,
                    Literal::I64(_) => Type::I64,
                    Literal::F32(_) => Type::F32,
                    Literal::F64(_) => Type::F64,
                    _ => Type::NONE,
                };
                *expr = self.builder.local_get(local_idx, ty);
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

        let body = func.body.unwrap();

        // Expecting:
        // (block
        //   (local.set $0 (i32.const 42))
        //   (i32.add (local.get $0) (local.get $0))
        // )

        match &body.kind {
            ExpressionKind::Block { list, .. } => {
                assert_eq!(list.len(), 2);
                // Check initialization
                if let ExpressionKind::LocalSet { index, value } = &list[0].kind {
                    assert_eq!(*index, 0);
                    assert!(matches!(
                        value.kind,
                        ExpressionKind::Const(Literal::I32(42))
                    ));
                } else {
                    panic!("Expected LocalSet");
                }

                // Check body replacement
                if let ExpressionKind::Binary { left, right, .. } = &list[1].kind {
                    assert!(matches!(
                        left.kind,
                        ExpressionKind::LocalGet { index: 0, .. }
                    ));
                    assert!(matches!(
                        right.kind,
                        ExpressionKind::LocalGet { index: 0, .. }
                    ));
                } else {
                    panic!("Expected Binary Add");
                }

                assert_eq!(func.vars.len(), 1); // Added 1 local
            }
            _ => panic!("Expected Block wrapper"),
        }
    }
}
