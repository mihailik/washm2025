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

                // Preserve any existing child block name if present (search recursively)
                let mut chosen_name: Option<String> = None;
                let mut search_stack = vec![set_ptr, bounds_check, new_load];
                while let Some(id) = search_stack.pop() {
                    let expr = &arena[id];
                    if let ExpressionKind::Block {
                        name: Some(ref n), ..
                    } = &expr.kind
                    {
                        chosen_name = Some(n.clone());
                        break;
                    }
                    expr.kind.for_each_child(|c| search_stack.push(c));
                }
                Some(ExpressionKind::Block {
                    name: chosen_name,
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

                // Preserve any existing child block name if present (search recursively)
                let mut chosen_name: Option<String> = None;
                let mut search_stack = vec![set_ptr, bounds_check, new_store];
                while let Some(id) = search_stack.pop() {
                    let expr = &arena[id];
                    if let ExpressionKind::Block {
                        name: Some(ref n), ..
                    } = &expr.kind
                    {
                        chosen_name = Some(n.clone());
                        break;
                    }
                    expr.kind.for_each_child(|c| search_stack.push(c));
                }
                Some(ExpressionKind::Block {
                    name: chosen_name,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::IrBuilder;
    use crate::ir::module::Function;
    use crate::ir::Literal;

    #[test]
    fn test_safe_heap_preserves_store_name() {
        let mut module = Module::new();
        let mut builder = IrBuilder::new(&mut module.expressions);

        // Create a value block with a name that should be preserved when SafeHeap wraps it
        let const_val = builder.const_(Literal::I32(42));
        let inner_block = builder.block(Some("inner"), vec![const_val], Type::I32);
        let ptr = builder.const_(Literal::I32(0));

        let store = builder.store(MemoryId::from_raw_parts(0, 0), 4, 0, 1, ptr, inner_block);

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            store,
        ));

        let mut pass = SafeHeap::new(LocalId::from_raw_parts(0, 0));
        pass.run(&mut module);

        let func_id = module.functions.ids().next().unwrap();
        let body_id = module.functions[func_id].body().unwrap();

        // After the pass, the function body should be a block that preserves the 'inner' name
        if let ExpressionKind::Block { name, list } = &module.expressions[body_id].kind {
            assert!(
                name == &Some(String::from("inner"))
                    || list.iter().any(|&id| match &module.expressions[id].kind {
                        ExpressionKind::Block { name: Some(n), .. } if n == "inner" => true,
                        _ => false,
                    })
            );
        } else {
            panic!("Expected Block, got {:?}", module.expressions[body_id].kind);
        }
    }
}
