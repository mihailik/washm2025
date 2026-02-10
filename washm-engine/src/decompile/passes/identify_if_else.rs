use crate::analysis::cfg::ControlFlowGraph;
use crate::ir::annotation::Annotation;
use crate::ir::expression::{ExpressionId, ExpressionKind};
use crate::ir::module::{FunctionKind, Module};
use std::collections::HashMap;

/// A pass that identifies 'If-Else' patterns in high-level control flow.
pub struct IdentifyIfElse;

impl Default for IdentifyIfElse {
    fn default() -> Self {
        Self::new()
    }
}

impl IdentifyIfElse {
    pub fn new() -> Self {
        Self
    }

    pub fn run(&self, module: &mut Module) {
        let mut results = HashMap::new();

        for (_, func) in &module.functions {
            if let FunctionKind::Local { body, .. } = &func.kind {
                let cfg = ControlFlowGraph::build(func, *body, &module.expressions);
                self.visit_expression(*body, &cfg, module, &mut results);
            }
        }

        // Apply annotations
        for (expr, ann) in results {
            module.annotations.insert(expr, ann);
        }
    }

    fn visit_expression(
        &self,
        expr_id: ExpressionId,
        cfg: &ControlFlowGraph,
        module: &Module,
        results: &mut HashMap<ExpressionId, Annotation>,
    ) {
        let expr = &module.expressions[expr_id];
        match &expr.kind {
            ExpressionKind::Block { list, .. } => {
                if !list.is_empty() {
                    let first_id = list[0];
                    if let ExpressionKind::Break {
                        target,
                        condition: Some(cond),
                        ..
                    } = &module.expressions[first_id].kind
                    {
                        if *target == expr_id {
                            // Double check with CFG: Does this br_if target the end of this block?
                            if let Some(&block_id) = cfg.expr_to_block.get(&first_id) {
                                let bb = &cfg.blocks[block_id];
                                if bb.succs.len() == 2 {
                                    results.insert(
                                        expr_id,
                                        Annotation::If {
                                            condition: *cond,
                                            inverted: true,
                                        },
                                    );
                                }
                            }
                        }
                    }
                }
                for child_id in list.iter() {
                    self.visit_expression(*child_id, cfg, module, results);
                }
            }
            _ => {
                // Visit sub-expressions using the expression's own traversal logic
                expr.kind.for_each_child(|child_id| {
                    self.visit_expression(child_id, cfg, module, results);
                });
            }
        }
    }
}
