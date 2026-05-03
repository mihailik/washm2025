use crate::ir::annotation::{Annotation, HighLevelType};
use crate::ir::expression::{ExpressionId, ExpressionKind};
use crate::ir::module::{FunctionKind, Module};
use crate::ir::ops::BinaryOp;
use crate::ir::visitor::ReadOnlyVisitor;

/// A pass that identifies expressions that are likely memory pointers.
pub struct IdentifyPointers;

impl Default for IdentifyPointers {
    fn default() -> Self {
        Self::new()
    }
}

impl IdentifyPointers {
    pub fn new() -> Self {
        Self
    }

    pub fn run(&mut self, module: &mut Module) {
        let mut pointers = Vec::new();

        for (_, func) in &module.functions {
            if let FunctionKind::Local { body, .. } = &func.kind {
                let mut visitor = SinkVisitor {
                    pointers: &mut pointers,
                };
                visitor.visit(*body, &module.expressions);
            }
        }

        // Propagate pointers backwards
        for expr in pointers {
            self.propagate_pointer(expr, module);
        }
    }

    fn propagate_pointer(&self, expr: ExpressionId, module: &mut Module) {
        if let Some(ann) = module.annotations.get(expr) {
            if ann.high_level_type == Some(HighLevelType::Pointer) {
                return;
            }
        }

        module
            .annotations
            .insert(expr, Annotation::Type(HighLevelType::Pointer));

        if let ExpressionKind::Binary { op, left, .. } = &module.expressions[expr].kind {
            if *op == BinaryOp::AddInt32 || *op == BinaryOp::SubInt32 {
                // In pointer arithmetic, usually the first operand is the pointer
                self.propagate_pointer(*left, module);
            }
        }
    }
}

struct SinkVisitor<'a> {
    pointers: &'a mut Vec<ExpressionId>,
}

impl<'a> ReadOnlyVisitor for SinkVisitor<'a> {
    fn visit_expression(
        &mut self,
        expr: ExpressionId,
        arena: &crate::collections::id_arena::IdArena<
            crate::ir::expression::ExpressionMarker,
            crate::ir::expression::Expression,
        >,
    ) {
        match &arena[expr].kind {
            ExpressionKind::Load { ptr, .. } | ExpressionKind::Store { ptr, .. } => {
                self.pointers.push(*ptr);
            }
            _ => {}
        }
    }
}
