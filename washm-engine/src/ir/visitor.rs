use crate::ir::expression::{ExprRef, ExpressionKind};

pub trait Visitor<'a> {
    fn visit(&mut self, expr: &mut ExprRef<'a>) {
        self.visit_expression(expr);
        self.visit_children(expr);
    }

    fn visit_expression(&mut self, _expr: &mut ExprRef<'a>) {}

    fn visit_children(&mut self, expr: &mut ExprRef<'a>) {
        match &mut expr.kind {
            ExpressionKind::Block { list, .. } => {
                for child in list.iter_mut() {
                    self.visit(child);
                }
            }
            ExpressionKind::Unary { value, .. } => {
                self.visit(value);
            }
            ExpressionKind::Binary { left, right, .. } => {
                self.visit(left);
                self.visit(right);
            }
            ExpressionKind::Call { operands, .. } => {
                for operand in operands.iter_mut() {
                    self.visit(operand);
                }
            }
            ExpressionKind::LocalSet { value, .. } | ExpressionKind::LocalTee { value, .. } => {
                self.visit(value);
            }
            ExpressionKind::GlobalSet { value, .. } => {
                self.visit(value);
            }
            ExpressionKind::If {
                condition,
                if_true,
                if_false,
            } => {
                self.visit(condition);
                self.visit(if_true);
                if let Some(false_branch) = if_false {
                    self.visit(false_branch);
                }
            }
            ExpressionKind::Loop { body, .. } => {
                self.visit(body);
            }
            ExpressionKind::Break {
                condition, value, ..
            } => {
                if let Some(cond) = condition {
                    self.visit(cond);
                }
                if let Some(val) = value {
                    self.visit(val);
                }
            }
            ExpressionKind::Return { value } => {
                if let Some(val) = value {
                    self.visit(val);
                }
            }
            ExpressionKind::Drop { value } => {
                self.visit(value);
            }
            ExpressionKind::Select {
                condition,
                if_true,
                if_false,
            } => {
                self.visit(condition);
                self.visit(if_true);
                self.visit(if_false);
            }
            ExpressionKind::Load { ptr, .. } => {
                self.visit(ptr);
            }
            ExpressionKind::Store { ptr, value, .. } => {
                self.visit(ptr);
                self.visit(value);
            }
            ExpressionKind::Switch {
                condition, value, ..
            } => {
                self.visit(condition);
                if let Some(val) = value {
                    self.visit(val);
                }
            }
            ExpressionKind::CallIndirect {
                target, operands, ..
            } => {
                self.visit(target);
                for operand in operands.iter_mut() {
                    self.visit(operand);
                }
            }
            ExpressionKind::MemoryGrow { delta } => {
                self.visit(delta);
            }
            ExpressionKind::AtomicRMW { ptr, value, .. } => {
                self.visit(ptr);
                self.visit(value);
            }
            ExpressionKind::AtomicCmpxchg {
                ptr,
                expected,
                replacement,
                ..
            } => {
                self.visit(ptr);
                self.visit(expected);
                self.visit(replacement);
            }
            ExpressionKind::AtomicWait {
                ptr,
                expected,
                timeout,
                ..
            } => {
                self.visit(ptr);
                self.visit(expected);
                self.visit(timeout);
            }
            ExpressionKind::AtomicNotify { ptr, count, .. } => {
                self.visit(ptr);
                self.visit(count);
            }
            ExpressionKind::SIMDExtract { vec, .. } => {
                self.visit(vec);
            }
            ExpressionKind::SIMDReplace { vec, value, .. } => {
                self.visit(vec);
                self.visit(value);
            }
            ExpressionKind::SIMDShuffle { left, right, .. } => {
                self.visit(left);
                self.visit(right);
            }
            ExpressionKind::SIMDTernary { a, b, c, .. } => {
                self.visit(a);
                self.visit(b);
                self.visit(c);
            }
            ExpressionKind::SIMDShift { vec, shift, .. } => {
                self.visit(vec);
                self.visit(shift);
            }
            ExpressionKind::SIMDLoad { ptr, .. } => {
                self.visit(ptr);
            }
            ExpressionKind::SIMDLoadStoreLane { ptr, vec, .. } => {
                self.visit(ptr);
                self.visit(vec);
            }
            ExpressionKind::MemoryInit {
                dest, offset, size, ..
            } => {
                self.visit(dest);
                self.visit(offset);
                self.visit(size);
            }
            ExpressionKind::MemoryCopy {
                dest, src, size, ..
            } => {
                self.visit(dest);
                self.visit(src);
                self.visit(size);
            }
            ExpressionKind::MemoryFill {
                dest, value, size, ..
            } => {
                self.visit(dest);
                self.visit(value);
                self.visit(size);
            }
            ExpressionKind::TableGet { index, .. } => {
                self.visit(index);
            }
            ExpressionKind::TableSet { index, value, .. } => {
                self.visit(index);
                self.visit(value);
            }
            ExpressionKind::TableGrow { delta, value, .. } => {
                self.visit(delta);
                self.visit(value);
            }
            ExpressionKind::TableFill {
                dest, value, size, ..
            } => {
                self.visit(dest);
                self.visit(value);
                self.visit(size);
            }
            ExpressionKind::TableCopy {
                dest, src, size, ..
            } => {
                self.visit(dest);
                self.visit(src);
                self.visit(size);
            }
            ExpressionKind::TableInit {
                dest, offset, size, ..
            } => {
                self.visit(dest);
                self.visit(offset);
                self.visit(size);
            }
            ExpressionKind::RefIsNull { value } => {
                self.visit(value);
            }
            ExpressionKind::RefAs { value, .. } => {
                self.visit(value);
            }
            ExpressionKind::RefEq { left, right } => {
                self.visit(left);
                self.visit(right);
            }
            ExpressionKind::StructNew { operands, .. } => {
                for operand in operands.iter_mut() {
                    self.visit(operand);
                }
            }
            ExpressionKind::StructGet { ptr, .. } => {
                self.visit(ptr);
            }
            ExpressionKind::StructSet { ptr, value, .. } => {
                self.visit(ptr);
                self.visit(value);
            }
            ExpressionKind::ArrayNew { size, init, .. } => {
                self.visit(size);
                if let Some(val) = init {
                    self.visit(val);
                }
            }
            ExpressionKind::ArrayGet { ptr, index, .. } => {
                self.visit(ptr);
                self.visit(index);
            }
            ExpressionKind::ArraySet {
                ptr, index, value, ..
            } => {
                self.visit(ptr);
                self.visit(index);
                self.visit(value);
            }
            ExpressionKind::ArrayLen { ptr } => {
                self.visit(ptr);
            }
            ExpressionKind::Try {
                body, catch_bodies, ..
            } => {
                self.visit(body);
                for catch_body in catch_bodies.iter_mut() {
                    self.visit(catch_body);
                }
            }
            ExpressionKind::Throw { operands, .. } => {
                for operand in operands.iter_mut() {
                    self.visit(operand);
                }
            }
            ExpressionKind::TupleMake { operands } => {
                for operand in operands.iter_mut() {
                    self.visit(operand);
                }
            }
            ExpressionKind::TupleExtract { tuple, .. } => {
                self.visit(tuple);
            }
            ExpressionKind::Unreachable
            | ExpressionKind::Const(_)
            | ExpressionKind::Nop
            | ExpressionKind::LocalGet { .. }
            | ExpressionKind::GlobalGet { .. }
            | ExpressionKind::MemorySize
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
                self.visit(value);
            }
        }
    }
}

pub trait ReadOnlyVisitor<'a> {
    fn visit(&mut self, expr: ExprRef<'a>) {
        self.visit_expression(expr);
        self.visit_children(expr);
    }

    fn visit_expression(&mut self, _expr: ExprRef<'a>) {}

    fn visit_children(&mut self, expr: ExprRef<'a>) {
        match &expr.kind {
            ExpressionKind::Block { list, .. } => {
                for child in list.iter() {
                    self.visit(*child);
                }
            }
            ExpressionKind::Unary { value, .. } => {
                self.visit(*value);
            }
            ExpressionKind::Binary { left, right, .. } => {
                self.visit(*left);
                self.visit(*right);
            }
            ExpressionKind::Call { operands, .. } => {
                for operand in operands.iter() {
                    self.visit(*operand);
                }
            }
            ExpressionKind::LocalSet { value, .. } | ExpressionKind::LocalTee { value, .. } => {
                self.visit(*value);
            }
            ExpressionKind::GlobalSet { value, .. } => {
                self.visit(*value);
            }
            ExpressionKind::If {
                condition,
                if_true,
                if_false,
            } => {
                self.visit(*condition);
                self.visit(*if_true);
                if let Some(false_branch) = if_false {
                    self.visit(*false_branch);
                }
            }
            ExpressionKind::Loop { body, .. } => {
                self.visit(*body);
            }
            ExpressionKind::Break {
                condition, value, ..
            } => {
                if let Some(cond) = condition {
                    self.visit(*cond);
                }
                if let Some(val) = value {
                    self.visit(*val);
                }
            }
            ExpressionKind::Return { value } => {
                if let Some(val) = value {
                    self.visit(*val);
                }
            }
            ExpressionKind::Drop { value } => {
                self.visit(*value);
            }
            ExpressionKind::Select {
                condition,
                if_true,
                if_false,
            } => {
                self.visit(*condition);
                self.visit(*if_true);
                self.visit(*if_false);
            }
            ExpressionKind::Load { ptr, .. } => {
                self.visit(*ptr);
            }
            ExpressionKind::Store { ptr, value, .. } => {
                self.visit(*ptr);
                self.visit(*value);
            }
            ExpressionKind::Switch {
                condition, value, ..
            } => {
                self.visit(*condition);
                if let Some(val) = value {
                    self.visit(*val);
                }
            }
            ExpressionKind::CallIndirect {
                target, operands, ..
            } => {
                self.visit(*target);
                for operand in operands.iter() {
                    self.visit(*operand);
                }
            }
            ExpressionKind::MemoryGrow { delta } => {
                self.visit(*delta);
            }
            ExpressionKind::AtomicRMW { ptr, value, .. } => {
                self.visit(*ptr);
                self.visit(*value);
            }
            ExpressionKind::AtomicCmpxchg {
                ptr,
                expected,
                replacement,
                ..
            } => {
                self.visit(*ptr);
                self.visit(*expected);
                self.visit(*replacement);
            }
            ExpressionKind::AtomicWait {
                ptr,
                expected,
                timeout,
                ..
            } => {
                self.visit(*ptr);
                self.visit(*expected);
                self.visit(*timeout);
            }
            ExpressionKind::AtomicNotify { ptr, count, .. } => {
                self.visit(*ptr);
                self.visit(*count);
            }
            ExpressionKind::SIMDExtract { vec, .. } => {
                self.visit(*vec);
            }
            ExpressionKind::SIMDReplace { vec, value, .. } => {
                self.visit(*vec);
                self.visit(*value);
            }
            ExpressionKind::SIMDShuffle { left, right, .. } => {
                self.visit(*left);
                self.visit(*right);
            }
            ExpressionKind::SIMDTernary { a, b, c, .. } => {
                self.visit(*a);
                self.visit(*b);
                self.visit(*c);
            }
            ExpressionKind::SIMDShift { vec, shift, .. } => {
                self.visit(*vec);
                self.visit(*shift);
            }
            ExpressionKind::SIMDLoad { ptr, .. } => {
                self.visit(*ptr);
            }
            ExpressionKind::SIMDLoadStoreLane { ptr, vec, .. } => {
                self.visit(*ptr);
                self.visit(*vec);
            }
            ExpressionKind::MemoryInit {
                dest, offset, size, ..
            } => {
                self.visit(*dest);
                self.visit(*offset);
                self.visit(*size);
            }
            ExpressionKind::MemoryCopy {
                dest, src, size, ..
            } => {
                self.visit(*dest);
                self.visit(*src);
                self.visit(*size);
            }
            ExpressionKind::MemoryFill {
                dest, value, size, ..
            } => {
                self.visit(*dest);
                self.visit(*value);
                self.visit(*size);
            }
            ExpressionKind::TableGet { index, .. } => {
                self.visit(*index);
            }
            ExpressionKind::TableSet { index, value, .. } => {
                self.visit(*index);
                self.visit(*value);
            }
            ExpressionKind::TableGrow { delta, value, .. } => {
                self.visit(*delta);
                self.visit(*value);
            }
            ExpressionKind::TableFill {
                dest, value, size, ..
            } => {
                self.visit(*dest);
                self.visit(*value);
                self.visit(*size);
            }
            ExpressionKind::TableCopy {
                dest, src, size, ..
            } => {
                self.visit(*dest);
                self.visit(*src);
                self.visit(*size);
            }
            ExpressionKind::TableInit {
                dest, offset, size, ..
            } => {
                self.visit(*dest);
                self.visit(*offset);
                self.visit(*size);
            }
            ExpressionKind::RefIsNull { value } => {
                self.visit(*value);
            }
            ExpressionKind::RefAs { value, .. } => {
                self.visit(*value);
            }
            ExpressionKind::RefEq { left, right } => {
                self.visit(*left);
                self.visit(*right);
            }
            ExpressionKind::StructNew { operands, .. } => {
                for operand in operands.iter() {
                    self.visit(*operand);
                }
            }
            ExpressionKind::StructGet { ptr, .. } => {
                self.visit(*ptr);
            }
            ExpressionKind::StructSet { ptr, value, .. } => {
                self.visit(*ptr);
                self.visit(*value);
            }
            ExpressionKind::ArrayNew { size, init, .. } => {
                self.visit(*size);
                if let Some(val) = init {
                    self.visit(*val);
                }
            }
            ExpressionKind::ArrayGet { ptr, index, .. } => {
                self.visit(*ptr);
                self.visit(*index);
            }
            ExpressionKind::ArraySet {
                ptr, index, value, ..
            } => {
                self.visit(*ptr);
                self.visit(*index);
                self.visit(*value);
            }
            ExpressionKind::ArrayLen { ptr } => {
                self.visit(*ptr);
            }
            ExpressionKind::Try {
                body, catch_bodies, ..
            } => {
                self.visit(*body);
                for catch_body in catch_bodies.iter() {
                    self.visit(*catch_body);
                }
            }
            ExpressionKind::Throw { operands, .. } => {
                for operand in operands.iter() {
                    self.visit(*operand);
                }
            }
            ExpressionKind::TupleMake { operands } => {
                for operand in operands.iter() {
                    self.visit(*operand);
                }
            }
            ExpressionKind::TupleExtract { tuple, .. } => {
                self.visit(*tuple);
            }
            ExpressionKind::Unreachable
            | ExpressionKind::Const(_)
            | ExpressionKind::Nop
            | ExpressionKind::LocalGet { .. }
            | ExpressionKind::GlobalGet { .. }
            | ExpressionKind::MemorySize
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
                self.visit(*value);
            }
        }
    }
}
