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
#[derive(Debug, Clone, Copy, Default)]
pub struct VariableStats {
    pub trait_scores: [u8; 8],
    pub name_hint_id: u32,
    pub hint_confidence: u8,
    _padding: [u8; 3],
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
        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();

        for (_, func) in functions.iter_mut() {
            if let FunctionKind::Local {
                body,
                locals,
                param_ids: _,
                var_ids: _,
            } = &mut func.kind
            {
                let mut stats: HashMap<crate::ir::LocalId, VariableStats> = HashMap::new();
                for (id, _) in locals.iter() {
                    stats.insert(id, VariableStats::default());
                }

                let mut visitor = NameInferenceVisitor { stats: &mut stats };
                visitor.visit(*body, arena);

                // Use BTreeMap or vec and sort so names are stable
                let mut sorted_stats: Vec<_> = stats.into_iter().collect();
                sorted_stats.sort_by_key(|(id, _)| *id);

                let mut name_counts = HashMap::new();

                for (local_id, variable_stats) in sorted_stats {
                    // Skip if name is already set from DWARF/Names section
                    if !locals[local_id].name.is_empty() {
                        continue;
                    }

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
                        let base_name = match best_trait {
                            TraitType::Index => "idx",
                            TraitType::Buffer => "ptr",
                            TraitType::Offset => "offset",
                            TraitType::Boolean => "flag",
                            TraitType::Length => "len",
                            TraitType::Bitmask => "mask",
                            TraitType::Handle => "h",
                            TraitType::Accumulator => "acc",
                        };

                        let count = name_counts.entry(base_name).or_insert(0);
                        let name = if *count == 0 {
                            base_name.to_string()
                        } else {
                            format!("{}{}", base_name, *count)
                        };
                        *count += 1;

                        locals[local_id].name = name;
                        locals[local_id].h_type = match best_trait {
                            TraitType::Buffer => {
                                Some(crate::ir::annotation::HighLevelType::Pointer)
                            }
                            TraitType::Boolean => Some(crate::ir::annotation::HighLevelType::Bool),
                            _ => None,
                        };
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
