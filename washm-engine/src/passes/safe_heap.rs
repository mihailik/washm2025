use crate::ir::expression::{ExprRef, Expression, ExpressionKind};
use crate::ir::module::Module;
use crate::ir::ops::BinaryOp;
use crate::ir::{Literal, Type};
use crate::passes::Pass;
use bumpalo::collections::Vec as BumpVec;

pub struct SafeHeap;

impl Pass for SafeHeap {
    fn name(&self) -> &str {
        "safe-heap"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        for func in &mut module.functions {
            if func.body.is_none() {
                continue;
            }

            // Calculate param count (Assuming MVP/single-value for now as Type iteration is not exposed)
            let param_count = if func.params == Type::NONE { 0 } else { 1 };

            let ptr_local_idx = func.vars.len() as u32 + param_count;
            func.vars.push(Type::I32); // ptr

            // Value locals: i32, i64, f32, f64. V128 not supported in this simple pass yet.
            let val_i32_idx = ptr_local_idx + 1;
            func.vars.push(Type::I32);
            let val_i64_idx = val_i32_idx + 1;
            func.vars.push(Type::I64);
            let val_f32_idx = val_i64_idx + 1;
            func.vars.push(Type::F32);
            let val_f64_idx = val_f32_idx + 1;
            func.vars.push(Type::F64);

            let mut rewriter = SafeHeapRewriter {
                ptr_local: ptr_local_idx,
                val_i32: val_i32_idx,
                val_i64: val_i64_idx,
                val_f32: val_f32_idx,
                val_f64: val_f64_idx,
                allocator: module.allocator,
            };

            if let Some(body) = &mut func.body {
                rewriter.visit_expr(body);
            }
        }
    }
}

struct SafeHeapRewriter<'a> {
    ptr_local: u32,
    val_i32: u32,
    val_i64: u32,
    val_f32: u32,
    val_f64: u32,
    allocator: &'a bumpalo::Bump,
}

impl<'a> SafeHeapRewriter<'a> {
    fn visit_expr(&mut self, expr: &mut ExprRef<'a>) {
        // First visit children to handle nested loads/stores
        self.visit_children(expr);

        let expr_type = expr.type_;

        // Now handle the expression itself if it is a Load or Store
        match &mut expr.kind {
            ExpressionKind::Load {
                ptr,
                offset,
                align,
                bytes,
                signed,
            } => {
                let ptr_expr = *ptr; // The original pointer expression
                let signed_val = *signed;

                // Construct new Load using local
                let local_get_ptr = self.allocator.alloc(Expression {
                    kind: ExpressionKind::LocalGet {
                        index: self.ptr_local,
                    },
                    type_: Type::I32,
                });

                let new_load = Expression {
                    kind: ExpressionKind::Load {
                        ptr: ExprRef::new(local_get_ptr),
                        offset: *offset,
                        align: *align,
                        bytes: *bytes,
                        signed: signed_val,
                    },
                    type_: expr_type,
                };

                let mut list = BumpVec::new_in(self.allocator);

                let local_set_ptr = self.allocator.alloc(Expression {
                    kind: ExpressionKind::LocalSet {
                        index: self.ptr_local,
                        value: ptr_expr,
                    },
                    type_: Type::NONE,
                });
                list.push(ExprRef::new(local_set_ptr));

                list.push(self.create_bounds_check(*offset, *bytes));

                let new_load_ref = self.allocator.alloc(new_load);
                list.push(ExprRef::new(new_load_ref));

                expr.kind = ExpressionKind::Block { name: None, list };
            }
            ExpressionKind::Store {
                ptr,
                value,
                offset,
                align,
                bytes,
            } => {
                let ptr_expr = *ptr;
                let value_expr = *value;
                let value_type = value_expr.type_;

                let val_local_idx = match value_type {
                    Type::I32 => self.val_i32,
                    Type::I64 => self.val_i64,
                    Type::F32 => self.val_f32,
                    Type::F64 => self.val_f64,
                    _ => return, // Skip unsupported types for now
                };

                let mut list = BumpVec::new_in(self.allocator);

                // 1. local.set  (ptr)
                let local_set_ptr = self.allocator.alloc(Expression {
                    kind: ExpressionKind::LocalSet {
                        index: self.ptr_local,
                        value: ptr_expr,
                    },
                    type_: Type::NONE,
                });
                list.push(ExprRef::new(local_set_ptr));

                // 2. local.set  (value)
                let local_set_val = self.allocator.alloc(Expression {
                    kind: ExpressionKind::LocalSet {
                        index: val_local_idx,
                        value: value_expr,
                    },
                    type_: Type::NONE,
                });
                list.push(ExprRef::new(local_set_val));

                // 3. check
                list.push(self.create_bounds_check(*offset, *bytes));

                // 4. store
                let get_ptr = self.allocator.alloc(Expression {
                    kind: ExpressionKind::LocalGet {
                        index: self.ptr_local,
                    },
                    type_: Type::I32,
                });
                let get_val = self.allocator.alloc(Expression {
                    kind: ExpressionKind::LocalGet {
                        index: val_local_idx,
                    },
                    type_: value_type,
                });

                let new_store = Expression {
                    kind: ExpressionKind::Store {
                        ptr: ExprRef::new(get_ptr),
                        value: ExprRef::new(get_val),
                        offset: *offset,
                        align: *align,
                        bytes: *bytes,
                    },
                    type_: Type::NONE,
                };

                let new_store_ref = self.allocator.alloc(new_store);
                list.push(ExprRef::new(new_store_ref));

                expr.kind = ExpressionKind::Block { name: None, list };
            }
            _ => {}
        }
    }

    fn create_bounds_check(&self, offset: u32, bytes: u32) -> ExprRef<'a> {
        // if (ptr + offset + bytes > mem_size * 64k) unreachable

        let get_ptr = self.allocator.alloc(Expression {
            kind: ExpressionKind::LocalGet {
                index: self.ptr_local,
            },
            type_: Type::I32,
        });

        let const_offset = self.allocator.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(offset as i32 + bytes as i32)),
            type_: Type::I32,
        });

        let effective_addr = self.allocator.alloc(Expression {
            kind: ExpressionKind::Binary {
                op: BinaryOp::AddInt32,
                left: ExprRef::new(get_ptr),
                right: ExprRef::new(const_offset),
            },
            type_: Type::I32,
        });

        // 2. Get memory size in bytes
        let mem_size_pages = self.allocator.alloc(Expression {
            kind: ExpressionKind::MemorySize,
            type_: Type::I32,
        });

        let const_page_size = self.allocator.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(65536)),
            type_: Type::I32,
        });

        let mem_size_bytes = self.allocator.alloc(Expression {
            kind: ExpressionKind::Binary {
                op: BinaryOp::MulInt32,
                left: ExprRef::new(mem_size_pages),
                right: ExprRef::new(const_page_size),
            },
            type_: Type::I32,
        });

        // 3. Compare: effective_addr > mem_size_bytes (unsigned)
        let condition = self.allocator.alloc(Expression {
            kind: ExpressionKind::Binary {
                op: BinaryOp::GtUInt32,
                left: ExprRef::new(effective_addr),
                right: ExprRef::new(mem_size_bytes),
            },
            type_: Type::I32,
        });

        // 4. If true, unreachable
        let unreachable = self.allocator.alloc(Expression {
            kind: ExpressionKind::Unreachable,
            type_: Type::UNREACHABLE,
        });

        let if_stmt = self.allocator.alloc(Expression {
            kind: ExpressionKind::If {
                condition: ExprRef::new(condition),
                if_true: ExprRef::new(unreachable),
                if_false: None,
            },
            type_: Type::NONE,
        });

        ExprRef::new(if_stmt)
    }

    fn visit_children(&mut self, expr: &mut ExprRef<'a>) {
        match &mut expr.kind {
            ExpressionKind::Block { list, .. } => {
                for child in list.iter_mut() {
                    self.visit_expr(child);
                }
            }
            ExpressionKind::Binary { left, right, .. }
            | ExpressionKind::Store {
                ptr: left,
                value: right,
                ..
            } => {
                self.visit_expr(left);
                self.visit_expr(right);
            }
            ExpressionKind::Unary { value, .. }
            | ExpressionKind::Load { ptr: value, .. }
            | ExpressionKind::Drop { value }
            | ExpressionKind::Loop { body: value, .. }
            | ExpressionKind::Return { value: Some(value) } => {
                self.visit_expr(value);
            }
            ExpressionKind::If {
                condition,
                if_true,
                if_false,
            } => {
                self.visit_expr(condition);
                self.visit_expr(if_true);
                if let Some(false_branch) = if_false {
                    self.visit_expr(false_branch);
                }
            }
            ExpressionKind::Call { operands, .. }
            | ExpressionKind::CallIndirect { operands, .. } => {
                for op in operands.iter_mut() {
                    self.visit_expr(op);
                }
            }
            ExpressionKind::LocalSet { value, .. }
            | ExpressionKind::LocalTee { value, .. }
            | ExpressionKind::GlobalSet { value, .. } => {
                self.visit_expr(value);
            }
            ExpressionKind::Select {
                condition,
                if_true,
                if_false,
            } => {
                self.visit_expr(condition);
                self.visit_expr(if_true);
                self.visit_expr(if_false);
            }
            ExpressionKind::Switch {
                condition, value, ..
            } => {
                self.visit_expr(condition);
                if let Some(v) = value {
                    self.visit_expr(v);
                }
            }
            ExpressionKind::MemoryGrow { delta } => {
                self.visit_expr(delta);
            }
            ExpressionKind::AtomicRMW { ptr, value, .. } => {
                self.visit_expr(ptr);
                self.visit_expr(value);
            }
            ExpressionKind::AtomicCmpxchg {
                ptr,
                expected,
                replacement,
                ..
            } => {
                self.visit_expr(ptr);
                self.visit_expr(expected);
                self.visit_expr(replacement);
            }
            ExpressionKind::AtomicWait {
                ptr,
                expected,
                timeout,
                ..
            } => {
                self.visit_expr(ptr);
                self.visit_expr(expected);
                self.visit_expr(timeout);
            }
            ExpressionKind::AtomicNotify { ptr, count } => {
                self.visit_expr(ptr);
                self.visit_expr(count);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::{ExprRef, Expression, ExpressionKind};
    use crate::ir::module::Function;
    use crate::ir::{Literal, Type};

    use bumpalo::Bump;

    #[test]
    fn test_safe_heap_run() {
        let allocator = Bump::new();
        let mut module = Module::new(&allocator);

        let load_ptr = allocator.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(0)),
            type_: Type::I32,
        });

        let load = allocator.alloc(Expression {
            kind: ExpressionKind::Load {
                bytes: 4,
                signed: false,
                offset: 0,
                align: 4,
                ptr: ExprRef::new(load_ptr),
            },
            type_: Type::I32,
        });

        let func = Function::new(
            "test_func".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(ExprRef::new(load)),
        );
        module.add_function(func);

        let mut pass = SafeHeap;
        pass.run(&mut module);

        assert!(module.get_function("test_func").is_some());

        // Inspect body to see if it's a Block now
        let func = module.get_function("test_func").unwrap();
        let body = func.body.as_ref().unwrap();

        match &body.kind {
            ExpressionKind::Block { list, .. } => {
                // Should have: local.set, check, load
                assert_eq!(list.len(), 3);
            }
            _ => panic!("Expected Block, got {:?}", body.kind),
        }
    }
}
