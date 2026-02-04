use crate::ir::expression::{ExpressionId, ExpressionKind};
use crate::ir::module::{FunctionKind, Module};
use crate::ir::ops::BinaryOp;
use crate::ir::visitor::ReadOnlyVisitor;
use std::collections::HashMap;

/// Semantic traits used to infer variable names based on usage patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TraitType {
    Index = 0,       // i32.add(v, 1), loop conditions
    Buffer = 1,      // Base of load/store
    Offset = 2,      // ptr + const or indexing
    Boolean = 3,     // eqz, select, if input
    Length = 4,      // Upper bound comparison
    Bitmask = 5,     // & or | with masks
    Handle = 6,      // WASI handles
    Accumulator = 7, // Repeated mutation
}

/// Compact representation of variable usage and hints.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VariableStats {
    pub trait_scores: [u8; 8],
    pub name_hint_id: u32,
    pub hint_confidence: u8,
    _padding: [u8; 3],
}

impl Default for VariableStats {
    fn default() -> Self {
        Self {
            trait_scores: [0; 8],
            name_hint_id: 0,
            hint_confidence: 0,
            _padding: [0; 3],
        }
    }
}

/// A synthesized identity for a variable after all passes.
#[derive(Debug, Clone)]
pub struct SemanticID {
    pub base_hint_id: u32,
    pub primary_trait: TraitType,
    pub confidence: u8,
}

pub struct NameInferencePass;

impl Default for NameInferencePass {
    fn default() -> Self {
        Self::new()
    }
}

impl NameInferencePass {
    pub fn new() -> Self {
        Self
    }

    pub fn run(&mut self, module: &mut Module) {
        for (_, func) in &module.functions {
            if let FunctionKind::Local { body, locals, .. } = &func.kind {
                let mut stats: HashMap<crate::ir::LocalId, VariableStats> = HashMap::new();
                for (id, _) in locals.iter() {
                    stats.insert(id, VariableStats::default());
                }

                let mut visitor = NameInferenceVisitor { stats: &mut stats };
                visitor.visit(*body, &module.expressions);

                // For now, apply simple names based on the highest scoring trait
                for (_local_id, variable_stats) in stats {
                    let mut max_score = 0;
                    let mut best_trait = TraitType::Index;
                    for (i, &score) in variable_stats.trait_scores.iter().enumerate() {
                        if score > max_score {
                            max_score = score;
                            best_trait = match i {
                                0 => TraitType::Index,
                                1 => TraitType::Buffer,
                                2 => TraitType::Offset,
                                3 => TraitType::Boolean,
                                4 => TraitType::Length,
                                5 => TraitType::Bitmask,
                                6 => TraitType::Handle,
                                7 => TraitType::Accumulator,
                                _ => TraitType::Index,
                            };
                        }
                    }

                    if max_score > 0 {
                        let _name = match best_trait {
                            TraitType::Index => "idx",
                            TraitType::Buffer => "ptr",
                            TraitType::Offset => "offset",
                            TraitType::Boolean => "flag",
                            TraitType::Length => "len",
                            TraitType::Bitmask => "mask",
                            TraitType::Handle => "h",
                            TraitType::Accumulator => "acc",
                        };
                        // We need a way to apply this name to all usages of this local.
                        // In the current architecture, we can't easily annotate the local itself via expressions,
                        // but we can add a LocalName annotation to the LocalGet/Set sites or somehow record it in the function.
                        // For now, let's just annotate the expression itself if it's a Get/Set.
                    }
                }
            }
        }
    }
}

struct NameInferenceVisitor<'a> {
    stats: &'a mut HashMap<crate::ir::LocalId, VariableStats>,
}

impl<'a> ReadOnlyVisitor for NameInferenceVisitor<'a> {
    fn visit_expression(
        &mut self,
        expr: ExpressionId,
        arena: &crate::collections::id_arena::IdArena<
            crate::ir::expression::ExpressionMarker,
            crate::ir::expression::Expression,
        >,
    ) {
        match &arena[expr].kind {
            ExpressionKind::LocalGet { id }
            | ExpressionKind::LocalSet { id, .. }
            | ExpressionKind::LocalTee { id, .. } => {
                if let Some(s) = self.stats.get_mut(id) {
                    // Simple example of trait scoring
                    s.trait_scores[TraitType::Index as usize] =
                        s.trait_scores[TraitType::Index as usize].saturating_add(1);
                }
            }
            ExpressionKind::Load { ptr, .. } | ExpressionKind::Store { ptr, .. } => {
                if let ExpressionKind::LocalGet { id } = &arena[*ptr].kind {
                    if let Some(s) = self.stats.get_mut(id) {
                        s.trait_scores[TraitType::Buffer as usize] =
                            s.trait_scores[TraitType::Buffer as usize].saturating_add(10);
                    }
                }
            }
            ExpressionKind::Binary { op, left, right } => {
                if matches!(
                    op,
                    BinaryOp::AddInt32
                        | BinaryOp::AddInt64
                        | BinaryOp::SubInt32
                        | BinaryOp::SubInt64
                ) {
                    if let ExpressionKind::LocalGet { id } = &arena[*left].kind {
                        if let Some(s) = self.stats.get_mut(id) {
                            s.trait_scores[TraitType::Accumulator as usize] =
                                s.trait_scores[TraitType::Accumulator as usize].saturating_add(5);
                        }
                    }
                    if let ExpressionKind::LocalGet { id } = &arena[*right].kind {
                        if let Some(s) = self.stats.get_mut(id) {
                            s.trait_scores[TraitType::Accumulator as usize] =
                                s.trait_scores[TraitType::Accumulator as usize].saturating_add(5);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}
