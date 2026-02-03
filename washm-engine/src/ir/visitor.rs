use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};

pub trait Visitor {
    fn visit(
        &mut self,
        expr: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        self.visit_expression(expr, arena);
        self.visit_children(expr, arena);
    }

    fn visit_expression(
        &mut self,
        _expr: &mut ExpressionId,
        _arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
    }

    fn visit_children(
        &mut self,
        expr: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        let mut kind = std::mem::replace(&mut arena[*expr].kind, ExpressionKind::Nop);
        kind.for_each_child_mut(|child_id| self.visit(child_id, arena));
        arena[*expr].kind = kind;
    }
}

pub trait ReadOnlyVisitor {
    fn visit(&mut self, expr: ExpressionId, arena: &IdArena<ExpressionMarker, Expression>) {
        self.visit_expression(expr, arena);
        self.visit_children(expr, arena);
    }

    fn visit_expression(
        &mut self,
        _expr: ExpressionId,
        _arena: &IdArena<ExpressionMarker, Expression>,
    ) {
    }

    fn visit_children(
        &mut self,
        expr: ExpressionId,
        arena: &IdArena<ExpressionMarker, Expression>,
    ) {
        match &arena[expr].kind {
            ExpressionKind::Block { list, .. } => {
                for child in list.iter() {
                    self.visit(*child, arena);
                }
            }
            ExpressionKind::Unary { value, .. } => {
                self.visit(*value, arena);
            }
            ExpressionKind::Binary { left, right, .. } => {
                self.visit(*left, arena);
                self.visit(*right, arena);
            }
            ExpressionKind::Call { operands, .. } => {
                for operand in operands.iter() {
                    self.visit(*operand, arena);
                }
            }
            ExpressionKind::LocalSet { value, .. } | ExpressionKind::LocalTee { value, .. } => {
                self.visit(*value, arena);
            }
            ExpressionKind::GlobalSet { value, .. } => {
                self.visit(*value, arena);
            }
            ExpressionKind::If {
                name: _,
                condition,
                if_true,
                if_false,
            } => {
                self.visit(*condition, arena);
                self.visit(*if_true, arena);
                if let Some(false_branch) = if_false {
                    self.visit(*false_branch, arena);
                }
            }
            ExpressionKind::Loop { body, .. } => {
                self.visit(*body, arena);
            }
            ExpressionKind::Break {
                condition, value, ..
            } => {
                if let Some(cond) = condition {
                    self.visit(*cond, arena);
                }
                if let Some(val) = value {
                    self.visit(*val, arena);
                }
            }
            ExpressionKind::Return { value } => {
                if let Some(val) = value {
                    self.visit(*val, arena);
                }
            }
            ExpressionKind::Drop { value } => {
                self.visit(*value, arena);
            }
            ExpressionKind::Select {
                condition,
                if_true,
                if_false,
            } => {
                self.visit(*condition, arena);
                self.visit(*if_true, arena);
                self.visit(*if_false, arena);
            }
            ExpressionKind::Load { ptr, .. } => {
                self.visit(*ptr, arena);
            }
            ExpressionKind::Store { ptr, value, .. } => {
                self.visit(*ptr, arena);
                self.visit(*value, arena);
            }
            ExpressionKind::Switch {
                condition, value, ..
            } => {
                self.visit(*condition, arena);
                if let Some(val) = value {
                    self.visit(*val, arena);
                }
            }
            ExpressionKind::CallIndirect {
                target, operands, ..
            } => {
                self.visit(*target, arena);
                for operand in operands.iter() {
                    self.visit(*operand, arena);
                }
            }
            ExpressionKind::MemoryGrow { delta, .. } => {
                self.visit(*delta, arena);
            }
            ExpressionKind::AtomicRMW { ptr, value, .. } => {
                self.visit(*ptr, arena);
                self.visit(*value, arena);
            }
            ExpressionKind::AtomicCmpxchg {
                ptr,
                expected,
                replacement,
                ..
            } => {
                self.visit(*ptr, arena);
                self.visit(*expected, arena);
                self.visit(*replacement, arena);
            }
            ExpressionKind::AtomicWait {
                ptr,
                expected,
                timeout,
                ..
            } => {
                self.visit(*ptr, arena);
                self.visit(*expected, arena);
                self.visit(*timeout, arena);
            }
            ExpressionKind::AtomicNotify { ptr, count, .. } => {
                self.visit(*ptr, arena);
                self.visit(*count, arena);
            }
            ExpressionKind::SIMDExtract { vec, .. } => {
                self.visit(*vec, arena);
            }
            ExpressionKind::SIMDReplace { vec, value, .. } => {
                self.visit(*vec, arena);
                self.visit(*value, arena);
            }
            ExpressionKind::SIMDShuffle { left, right, .. } => {
                self.visit(*left, arena);
                self.visit(*right, arena);
            }
            ExpressionKind::SIMDTernary { a, b, c, .. } => {
                self.visit(*a, arena);
                self.visit(*b, arena);
                self.visit(*c, arena);
            }
            ExpressionKind::SIMDShift { vec, shift, .. } => {
                self.visit(*vec, arena);
                self.visit(*shift, arena);
            }
            ExpressionKind::SIMDLoad { ptr, .. } => {
                self.visit(*ptr, arena);
            }
            ExpressionKind::SIMDLoadStoreLane { ptr, vec, .. } => {
                self.visit(*ptr, arena);
                self.visit(*vec, arena);
            }
            ExpressionKind::MemoryInit {
                dest, offset, size, ..
            } => {
                self.visit(*dest, arena);
                self.visit(*offset, arena);
                self.visit(*size, arena);
            }
            ExpressionKind::MemoryCopy {
                dest, src, size, ..
            } => {
                self.visit(*dest, arena);
                self.visit(*src, arena);
                self.visit(*size, arena);
            }
            ExpressionKind::MemoryFill {
                dest, value, size, ..
            } => {
                self.visit(*dest, arena);
                self.visit(*value, arena);
                self.visit(*size, arena);
            }
            ExpressionKind::TableGet { index, .. } => {
                self.visit(*index, arena);
            }
            ExpressionKind::TableSet { index, value, .. } => {
                self.visit(*index, arena);
                self.visit(*value, arena);
            }
            ExpressionKind::TableGrow { delta, value, .. } => {
                self.visit(*delta, arena);
                self.visit(*value, arena);
            }
            ExpressionKind::TableFill {
                dest, value, size, ..
            } => {
                self.visit(*dest, arena);
                self.visit(*value, arena);
                self.visit(*size, arena);
            }
            ExpressionKind::TableCopy {
                dest, src, size, ..
            } => {
                self.visit(*dest, arena);
                self.visit(*src, arena);
                self.visit(*size, arena);
            }
            ExpressionKind::TableInit {
                dest, offset, size, ..
            } => {
                self.visit(*dest, arena);
                self.visit(*offset, arena);
                self.visit(*size, arena);
            }
            ExpressionKind::RefIsNull { value } => {
                self.visit(*value, arena);
            }
            ExpressionKind::RefAs { value, .. } => {
                self.visit(*value, arena);
            }
            ExpressionKind::RefEq { left, right } => {
                self.visit(*left, arena);
                self.visit(*right, arena);
            }
            ExpressionKind::StructNew { operands, .. } => {
                for operand in operands.iter() {
                    self.visit(*operand, arena);
                }
            }
            ExpressionKind::StructGet { ptr, .. } => {
                self.visit(*ptr, arena);
            }
            ExpressionKind::StructSet { ptr, value, .. } => {
                self.visit(*ptr, arena);
                self.visit(*value, arena);
            }
            ExpressionKind::ArrayNew { size, init, .. } => {
                self.visit(*size, arena);
                if let Some(val) = init {
                    self.visit(*val, arena);
                }
            }
            ExpressionKind::ArrayGet { ptr, index, .. } => {
                self.visit(*ptr, arena);
                self.visit(*index, arena);
            }
            ExpressionKind::ArraySet {
                ptr, index, value, ..
            } => {
                self.visit(*ptr, arena);
                self.visit(*index, arena);
                self.visit(*value, arena);
            }
            ExpressionKind::ArrayLen { ptr } => {
                self.visit(*ptr, arena);
            }
            ExpressionKind::Try {
                body, catch_bodies, ..
            } => {
                self.visit(*body, arena);
                for catch_body in catch_bodies.iter() {
                    self.visit(*catch_body, arena);
                }
            }
            ExpressionKind::Throw { operands, .. } => {
                for operand in operands.iter() {
                    self.visit(*operand, arena);
                }
            }
            ExpressionKind::TupleMake { operands } => {
                for operand in operands.iter() {
                    self.visit(*operand, arena);
                }
            }
            ExpressionKind::TupleExtract { tuple, .. } => {
                self.visit(*tuple, arena);
            }
            ExpressionKind::Unreachable
            | ExpressionKind::Const(_)
            | ExpressionKind::Nop
            | ExpressionKind::LocalGet { .. }
            | ExpressionKind::GlobalGet { .. }
            | ExpressionKind::MemorySize { .. }
            | ExpressionKind::AtomicFence
            | ExpressionKind::DataDrop { .. }
            | ExpressionKind::TableSize { .. }
            | ExpressionKind::RefNull { .. }
            | ExpressionKind::RefFunc { .. }
            | ExpressionKind::ElemDrop { .. }
            | ExpressionKind::Rethrow { .. }
            | ExpressionKind::Pop { .. } => {}
            ExpressionKind::RefTest { value, .. }
            | ExpressionKind::RefCast { value, .. }
            | ExpressionKind::BrOn { value, .. }
            | ExpressionKind::I31New { value }
            | ExpressionKind::I31Get { i31: value, .. } => {
                self.visit(*value, arena);
            }
        }
    }
}
