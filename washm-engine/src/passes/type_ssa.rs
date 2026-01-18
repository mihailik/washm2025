use crate::analysis::cfg::ControlFlowGraph;
use crate::analysis::dominators::DominanceTree;
use crate::analysis::ssa::{DefA, SSABuilder};
use crate::ir::expression::ExprRef;
use crate::ir::module::Module;
use crate::passes::Pass;
use crate::ir::visitor::Visitor;
use crate::ir::Type;
use std::collections::HashMap;

/// Type SSA pass: Refines expression types using SSA information.
pub struct TypeSSA;

impl Pass for TypeSSA {
    fn name(&self) -> &str {
        "type-ssa"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        for func in &mut module.functions {
            if let Some(mut body) = func.body {
                // 1. Build SSA
                let cfg = ControlFlowGraph::build(func, body);
                let dom = DominanceTree::build(&cfg);
                let ssa = SSABuilder::build(func, &cfg, &dom);

                // 2. Refine types based on SSA definitions
                let mut refinements = HashMap::new();
                for (def, uses) in &ssa.def_uses {
                    let refined_type = match def {
                        DefA::Instruction(expr) => expr.type_,
                        DefA::Param(_idx) => {
                            // Parameters have a fixed declared type.
                            // But maybe in a whole-module context they can be refined.
                            // For now, keep as is.
                            continue;
                        }
                        DefA::Phi(_block, _local) => {
                            // A Phi's type is the LUB of its incoming definitions.
                            // This is where real refinement happens.
                            continue; // TODO: Implement LUB propagation for Phis
                        }
                    };

                    for &use_expr in uses {
                        if refined_type.is_subtype_of(use_expr.type_)
                            && refined_type != use_expr.type_
                        {
                            refinements.insert(use_expr.as_ptr(), refined_type);
                        }
                    }
                }

                // 3. Apply refinements
                let mut applier = TypeRefiner { refinements };
                applier.visit(&mut body);
                func.body = Some(body);
            }
        }
    }
}

struct TypeRefiner<'a> {
    refinements: HashMap<*mut crate::ir::expression::Expression<'a>, Type>,
}

impl<'a> Visitor<'a> for TypeRefiner<'a> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        self.visit_children(expr);

        // Safety: We use the pointer to lookup refinements.
        // The pointer is stable during the visit.
        if let Some(&new_type) = self.refinements.get(&(expr.as_ptr() as *mut _)) {
            expr.type_ = new_type;
        }
    }
}
