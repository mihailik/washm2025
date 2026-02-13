use crate::ir::annotation::Annotation;
use crate::ir::expression::{ExpressionId, ExpressionKind};
use crate::ir::module::{FunctionKind, Module};
use std::collections::HashMap;

pub struct ExpressionRecombination;

impl ExpressionRecombination {
    pub fn run(module: &mut Module) {
        for (_, func) in module.functions.iter_mut() {
            if let FunctionKind::Local { body, .. } = &func.kind {
                let mut analyzer = UsageAnalyzer::new();
                analyzer.visit(*body, &module.expressions, &module.annotations);

                let mut inliner = Inliner {
                    annotations: &mut module.annotations,
                    expressions: &module.expressions,
                    analyzer,
                };
                inliner.process(*body, false);
            }
        }
    }
}

struct UsageAnalyzer {
    set_counts: HashMap<crate::ir::LocalId, usize>,
    get_counts: HashMap<crate::ir::LocalId, usize>,
    last_set: HashMap<crate::ir::LocalId, ExpressionId>,
}

impl UsageAnalyzer {
    fn new() -> Self {
        Self {
            set_counts: HashMap::new(),
            get_counts: HashMap::new(),
            last_set: HashMap::new(),
        }
    }

    fn visit(
        &mut self,
        expr_id: ExpressionId,
        arena: &crate::collections::id_arena::IdArena<
            crate::ir::expression::ExpressionMarker,
            crate::ir::expression::Expression,
        >,
        annotations: &crate::ir::annotation::AnnotationStore,
    ) {
        match &arena[expr_id].kind {
            ExpressionKind::LocalSet { id, .. } | ExpressionKind::LocalTee { id, .. } => {
                *self.set_counts.entry(*id).or_insert(0) += 1;
                self.last_set.insert(*id, expr_id);
            }
            ExpressionKind::LocalGet { id } => {
                *self.get_counts.entry(*id).or_insert(0) += 1;
            }
            _ => {}
        }
        arena[expr_id]
            .kind
            .for_each_child(|child_id| self.visit(child_id, arena, annotations));
    }
}

struct Inliner<'a> {
    annotations: &'a mut crate::ir::annotation::AnnotationStore,
    expressions: &'a crate::collections::id_arena::IdArena<
        crate::ir::expression::ExpressionMarker,
        crate::ir::expression::Expression,
    >,
    analyzer: UsageAnalyzer,
}

impl<'a> Inliner<'a> {
    fn process(&mut self, expr_id: ExpressionId, in_condition: bool) {
        if !in_condition {
            self.annotate(expr_id);
        }

        let expr = &self.expressions[expr_id];
        match &expr.kind {
            ExpressionKind::If {
                condition,
                if_true,
                if_false,
                ..
            } => {
                self.process(*condition, true);
                self.process(*if_true, false);
                if let Some(f) = if_false {
                    self.process(*f, false);
                }
            }
            ExpressionKind::Break {
                condition, value, ..
            } => {
                if let Some(c) = condition {
                    self.process(*c, true);
                }
                if let Some(v) = value {
                    self.process(*v, false);
                }
            }
            ExpressionKind::Switch {
                condition, value, ..
            } => {
                self.process(*condition, true);
                if let Some(v) = value {
                    self.process(*v, false);
                }
            }
            ExpressionKind::Select {
                condition,
                if_true,
                if_false,
            } => {
                self.process(*condition, true);
                self.process(*if_true, false);
                self.process(*if_false, false);
            }
            ExpressionKind::Block { list, .. } => {
                if let Some((condition, _)) = self.annotations.get_if_info(expr_id) {
                    self.process(condition, true);
                    for &child in list.iter().skip(1) {
                        self.process(child, false);
                    }
                } else {
                    for &child in list {
                        self.process(child, false);
                    }
                }
            }
            _ => {
                expr.kind
                    .for_each_child(|child_id| self.process(child_id, in_condition));
            }
        }
    }

    fn annotate(&mut self, expr_id: ExpressionId) {
        if let ExpressionKind::LocalGet { id } = &self.expressions[expr_id].kind {
            if self.analyzer.get_counts.get(id) == Some(&1)
                && self.analyzer.set_counts.get(id) == Some(&1)
            {
                if let Some(&set_id) = self.analyzer.last_set.get(id) {
                    if let ExpressionKind::LocalSet { value, .. }
                    | ExpressionKind::LocalTee { value, .. } = &self.expressions[set_id].kind
                    {
                        self.annotations
                            .insert(expr_id, Annotation::InlinedValue(*value));
                        self.annotations.insert(set_id, Annotation::Inlined);
                    }
                }
            }
        }
    }
}
