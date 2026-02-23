use crate::ir::annotation::{Annotation, HighLevelType};
use crate::ir::expression::{ExpressionId, ExpressionKind};
use crate::ir::module::{FunctionKind, Module};
use crate::ir::ops::BinaryOp;
use crate::ir::visitor::ReadOnlyVisitor;
use std::collections::{HashMap, HashSet};

/// A pass that identifies structure-like access patterns from pointers.
pub struct IdentifyStructs;

impl Default for IdentifyStructs {
    fn default() -> Self {
        Self::new()
    }
}

impl IdentifyStructs {
    pub fn new() -> Self {
        Self
    }

    pub fn run(&mut self, module: &mut Module) {
        let mut all_annotations = Vec::new();

        for (_, func) in &mut module.functions {
            if let FunctionKind::Local { body, .. } = &func.kind {
                let mut visitor = StructVisitor {
                    arena: &module.expressions,
                    local_offsets: HashMap::new(),
                    local_usages: HashMap::new(),
                };
                visitor.visit_expression(*body, &module.expressions);

                // For each local that had structural access, annotate the usages.
                for (local_id, offsets) in visitor.local_offsets {
                    // Mark as pointer h_type
                    if let FunctionKind::Local { locals, .. } = &mut func.kind {
                        locals[local_id].h_type = Some(HighLevelType::Pointer);
                    }

                    // Annotate each use site
                    if let Some(usages) = visitor.local_usages.get(&local_id) {
                        for &usage_id in usages {
                            all_annotations
                                .push((usage_id, Annotation::StructOffsets(offsets.clone())));
                        }
                    }
                }
            }
        }

        // Apply annotations
        for (usage_id, ann) in all_annotations {
            module.annotations.insert(usage_id, ann);
        }
    }
}

struct StructVisitor<'a> {
    arena: &'a crate::collections::id_arena::IdArena<
        crate::ir::expression::ExpressionMarker,
        crate::ir::expression::Expression,
    >,
    // Mapping from LocalId to a set of (offset, size) used with it.
    pub local_offsets: HashMap<crate::ir::LocalId, HashSet<(u32, u32)>>,
    pub local_usages: HashMap<crate::ir::LocalId, Vec<ExpressionId>>,
}

impl<'a> StructVisitor<'a> {
    fn record_access(&mut self, ptr_id: ExpressionId, offset: u32, bytes: u32) {
        match &self.arena[ptr_id].kind {
            ExpressionKind::LocalGet { id } => {
                self.local_offsets
                    .entry(*id)
                    .or_default()
                    .insert((offset, bytes));
            }
            ExpressionKind::LocalTee { id, value } => {
                self.local_offsets
                    .entry(*id)
                    .or_default()
                    .insert((offset, bytes));
                self.record_access(*value, offset, bytes);
            }
            ExpressionKind::Binary { op, left, right } if *op == BinaryOp::AddInt32 => {
                if let ExpressionKind::Const(crate::ir::Literal::I32(val)) =
                    &self.arena[*right].kind
                {
                    self.record_access(*left, offset.wrapping_add(*val as u32), bytes);
                } else if let ExpressionKind::Const(crate::ir::Literal::I32(val)) =
                    &self.arena[*left].kind
                {
                    self.record_access(*right, offset.wrapping_add(*val as u32), bytes);
                }
            }
            _ => {}
        }
    }
}

impl<'a> ReadOnlyVisitor for StructVisitor<'a> {
    fn visit_expression(
        &mut self,
        expr_id: ExpressionId,
        arena: &crate::collections::id_arena::IdArena<
            crate::ir::expression::ExpressionMarker,
            crate::ir::expression::Expression,
        >,
    ) {
        match &arena[expr_id].kind {
            ExpressionKind::Load {
                ptr, offset, bytes, ..
            } => {
                self.record_access(*ptr, *offset, *bytes);
            }
            ExpressionKind::Store {
                ptr, offset, bytes, ..
            } => {
                self.record_access(*ptr, *offset, *bytes);
            }
            ExpressionKind::LocalGet { id } => {
                self.local_usages.entry(*id).or_default().push(expr_id);
            }
            ExpressionKind::LocalTee { id, .. } => {
                self.local_usages.entry(*id).or_default().push(expr_id);
            }
            _ => {}
        }
        arena[expr_id].kind.for_each_child(|child_id| {
            self.visit_expression(child_id, arena);
        });
    }
}
