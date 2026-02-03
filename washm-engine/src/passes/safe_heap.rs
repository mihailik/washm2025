use crate::collections::id_arena::IdArena;
use crate::ir::expression::{
    Expression, ExpressionId, ExpressionKind, ExpressionMarker, IrBuilder,
};
use crate::ir::module::Module;
use crate::ir::visitor::Visitor;
use crate::ir::{Literal, LocalId, MemoryId, Type};
use crate::passes::Pass;

/// Instruments all heap accesses with bounds checks.
pub struct SafeHeap {
    ptr_local: LocalId,
}

impl Default for SafeHeap {
    fn default() -> Self {
        Self {
            ptr_local: LocalId::from_raw_parts(0, 0),
        }
    }
}

impl SafeHeap {
    pub fn new(ptr_local: LocalId) -> Self {
        Self { ptr_local }
    }
}

impl Pass for SafeHeap {
    fn name(&self) -> &str {
        "safe-heap"
    }

    fn run(&mut self, module: &mut Module) {
        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        let mut visitor = SafeHeapVisitor {
            ptr_local: self.ptr_local,
        };

        for (_, func) in functions {
            if let Some(body) = func.body_mut() {
                visitor.visit(body, arena);
            }
        }
    }
}

struct SafeHeapVisitor {
    ptr_local: LocalId,
}

impl SafeHeapVisitor {
    fn create_bounds_check(
        &self,
        builder: &mut IrBuilder,
        offset: u32,
        bytes: u32,
    ) -> ExpressionId {
        // Simple bounds check: (ptr + offset + bytes) <= memory_size
        let ptr = builder.local_get(self.ptr_local, Type::I32);
        let offset_lit = builder.const_(Literal::I32(offset as i32));
        let bytes_lit = builder.const_(Literal::I32(bytes as i32));

        let total = builder.binary(
            crate::ir::ops::BinaryOp::AddInt32,
            ptr,
            offset_lit,
            Type::I32,
        );
        let total = builder.binary(
            crate::ir::ops::BinaryOp::AddInt32,
            total,
            bytes_lit,
            Type::I32,
        );

        let mem_size = builder.memory_size(MemoryId::from_raw_parts(0, 0));
        let page_size = builder.const_(Literal::I32(65536));
        let mem_size_bytes = builder.binary(
            crate::ir::ops::BinaryOp::MulInt32,
            mem_size,
            page_size,
            Type::I32,
        );

        let check = builder.binary(
            crate::ir::ops::BinaryOp::LeUInt32,
            total,
            mem_size_bytes,
            Type::I32,
        );

        let nop = builder.nop();
        let unreachable = builder.unreachable();
        builder.if_("", check, nop, Some(unreachable), Type::NONE)
    }
}

impl Visitor for SafeHeapVisitor {
    fn visit(
        &mut self,
        expr_id: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        self.visit_children(expr_id, arena);

        let new_node = match &arena[*expr_id].kind {
            ExpressionKind::Load {
                ptr,
                offset,
                align,
                bytes,
                signed,
                memory,
            } => {
                let expr_type = arena[*expr_id].type_;
                let ptr_val = *ptr;
                let offset_val = *offset;
                let align_val = *align;
                let bytes_val = *bytes;
                let signed_val = *signed;
                let mem_id = *memory;

                let mut builder = IrBuilder::new(arena);

                let set_ptr = builder.local_set(self.ptr_local, ptr_val);
                let bounds_check = self.create_bounds_check(&mut builder, offset_val, bytes_val);
                let get_ptr = builder.local_get(self.ptr_local, Type::I32);
                let new_load = builder.load(
                    mem_id, bytes_val, signed_val, offset_val, align_val, get_ptr, expr_type,
                );

                Some(ExpressionKind::Block {
                    name: None,
                    list: vec![set_ptr, bounds_check, new_load],
                })
            }
            ExpressionKind::Store {
                ptr,
                value,
                offset,
                align,
                bytes,
                memory,
            } => {
                let ptr_val = *ptr;
                let val_val = *value;
                let offset_val = *offset;
                let align_val = *align;
                let bytes_val = *bytes;
                let mem_id = *memory;

                let mut builder = IrBuilder::new(arena);

                let set_ptr = builder.local_set(self.ptr_local, ptr_val);
                let bounds_check = self.create_bounds_check(&mut builder, offset_val, bytes_val);
                let get_ptr = builder.local_get(self.ptr_local, Type::I32);
                let new_store =
                    builder.store(mem_id, bytes_val, offset_val, align_val, get_ptr, val_val);

                Some(ExpressionKind::Block {
                    name: None,
                    list: vec![set_ptr, bounds_check, new_store],
                })
            }
            _ => None,
        };

        if let Some(kind) = new_node {
            arena[*expr_id].kind = kind;
        }
    }
}
