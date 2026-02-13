use crate::ir::annotation::{Annotation, LoopType};
use crate::ir::expression::{ExpressionId, ExpressionKind};
use crate::ir::module::{FunctionKind, Module};
use crate::ir::visitor::ReadOnlyVisitor;

/// A pass that identifies loop types (While, Do-While) based on branch patterns.
pub struct IdentifyLoops;

impl Default for IdentifyLoops {
    fn default() -> Self {
        Self::new()
    }
}

impl IdentifyLoops {
    pub fn new() -> Self {
        Self
    }

    pub fn run(&mut self, module: &mut Module) {
        let mut loop_annotations = Vec::new();

        for (_, func) in &module.functions {
            if let FunctionKind::Local { body, .. } = &func.kind {
                let mut visitor = LoopVisitor {
                    loop_annotations: &mut loop_annotations,
                };
                visitor.visit(*body, &module.expressions);
            }
        }

        for (expr, loop_type) in loop_annotations {
            module.annotations.insert(expr, Annotation::Loop(loop_type));
        }
    }
}

struct LoopVisitor<'a> {
    loop_annotations: &'a mut Vec<(ExpressionId, LoopType)>,
}

impl<'a> ReadOnlyVisitor for LoopVisitor<'a> {
    fn visit_expression(
        &mut self,
        expr: ExpressionId,
        arena: &crate::collections::id_arena::IdArena<
            crate::ir::expression::ExpressionMarker,
            crate::ir::expression::Expression,
        >,
    ) {
        if let ExpressionKind::Loop { name: _, body } = &arena[expr].kind {
            let mut detected = false;

            // 1. Look for Do-While Pattern:
            // (loop $L (block ... (br_if $L (cond))))
            if let ExpressionKind::Block { list, .. } = &arena[*body].kind {
                if let Some(last_id) = list.last() {
                    if let ExpressionKind::Break {
                        target,
                        condition: Some(_),
                        ..
                    } = &arena[*last_id].kind
                    {
                        if *target == expr {
                            self.loop_annotations.push((expr, LoopType::DoWhile));
                            detected = true;
                        }
                    }
                }
            }

            // 2. Look for While Pattern:
            // (loop $L (if (cond) (block ... (br $L))))
            if !detected {
                if let ExpressionKind::If { .. } = &arena[*body].kind {
                    self.loop_annotations.push((expr, LoopType::While));
                }
            }
        }
    }
}
