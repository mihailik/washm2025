use crate::analysis::cfg::ControlFlowGraph;
use crate::analysis::dominators::DominanceTree;
use crate::analysis::ssa::{DefA, SSABuilder};
use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionMarker};
use crate::ir::module::Module;
use crate::ir::visitor::Visitor;
use crate::ir::Type;
use crate::passes::Pass;
use std::collections::HashMap;

/// Type SSA pass: Refines expression types using SSA information.
pub struct TypeSSA;

impl Pass for TypeSSA {
    fn name(&self) -> &str {
        "type-ssa"
    }

    fn run(&mut self, module: &mut Module) {
        let arena = &module.expressions;
        let mut all_refinements = Vec::new();

        for (_, func) in &module.functions {
            if let Some(body) = func.body() {
                // 1. Build SSA
                let cfg = ControlFlowGraph::build(func, body, arena);
                let dom = DominanceTree::build(&cfg);
                let ssa = SSABuilder::build(func, &cfg, &dom, arena);

                // 2. Refine types based on SSA definitions
                let mut refinements = HashMap::new();
                for (def, uses) in &ssa.def_uses {
                    let refined_type = match def {
                        DefA::Instruction(expr_id) => arena[*expr_id].type_,
                        DefA::Param(_idx) => {
                            // Parameters have a fixed declared type.
                            continue;
                        }
                        DefA::Phi(_block, _local) => {
                            // A Phi's type is the LUB of its incoming definitions.
                            continue; // TODO: Implement LUB propagation for Phis
                        }
                    };

                    for &use_expr_id in uses {
                        let use_expr = &arena[use_expr_id];
                        if refined_type.is_subtype_of(use_expr.type_)
                            && refined_type != use_expr.type_
                        {
                            refinements.insert(use_expr_id, refined_type);
                        }
                    }
                }
                if !refinements.is_empty() {
                    all_refinements.push((body, refinements));
                }
            }
        }

        // 3. Apply refinements
        for (mut body_id, refinements) in all_refinements {
            let mut applier = TypeRefiner { refinements };
            applier.visit(&mut body_id, &mut module.expressions);
        }
    }
}

struct TypeRefiner {
    refinements: HashMap<ExpressionId, Type>,
}

impl Visitor for TypeRefiner {
    fn visit_expression(
        &mut self,
        expr_id: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        self.visit_children(expr_id, arena);

        if let Some(&new_type) = self.refinements.get(expr_id) {
            arena[*expr_id].type_ = new_type;
        }
    }
}
