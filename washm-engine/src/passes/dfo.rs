use crate::analysis::cfg::ControlFlowGraph;
use crate::analysis::dominators::DominanceTree;
use crate::analysis::effects::EffectAnalyzer;
use crate::analysis::ssa::{DefA, SSABuilder};
use crate::ir::expression::{ExprRef, ExpressionKind};
use crate::ir::module::Module;
use crate::ir::visitor::Visitor;
use crate::passes::Pass;
use std::collections::{HashMap, HashSet};

/// Data Flow Optimizations
pub struct DataFlowOpts;

impl Pass for DataFlowOpts {
    fn name(&self) -> &str {
        "dfo"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        for func in &mut module.functions {
            if let Some(mut body) = func.body {
                // 1. Build CFG
                let cfg = ControlFlowGraph::build(func, body);

                // 2. Build Dominator Tree (required for SSA)
                let dom = DominanceTree::build(&cfg);

                // 3. Build SSA info (including Def-Use chains)
                let ssa = SSABuilder::build(func, &cfg, &dom);

                // 4. Optimization Phase: Use SSA to find refinements
                let mut dead_definitions = HashSet::new();
                let mut constant_props: HashMap<
                    *mut crate::ir::expression::Expression<'a>,
                    ExprRef<'a>,
                > = HashMap::new();

                // Dead Store Elimination and Constant Propagation
                for (def, uses) in &ssa.def_uses {
                    match def {
                        DefA::Instruction(instr) => {
                            if let ExpressionKind::LocalSet { value, .. }
                            | ExpressionKind::LocalTee { value, .. } = &instr.kind
                            {
                                // Dead Store Elimination: No uses and no side effects
                                if uses.is_empty() {
                                    let effects = EffectAnalyzer::analyze(*value);
                                    if !effects.has_side_effects() {
                                        dead_definitions.insert(instr.as_ptr());
                                    }
                                }

                                // Simple Constant Propagation: If value is a constant, propagate to all uses
                                if let ExpressionKind::Const { .. } = &value.kind {
                                    for &use_expr in uses {
                                        constant_props.insert(use_expr.as_ptr(), *value);
                                    }
                                }

                                // Generalized Copy Propagation: local.set $v2, (local.get $v1)
                                if let ExpressionKind::LocalGet { .. } = &value.kind {
                                    for &use_expr in uses {
                                        constant_props.insert(use_expr.as_ptr(), *value);
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }

                // 5. Apply transformations
                let mut applier = DfoApplier {
                    dead_definitions,
                    constant_props,
                };
                applier.visit(&mut body);
            }
        }
    }
}

struct DfoApplier<'a> {
    dead_definitions: HashSet<*mut crate::ir::expression::Expression<'a>>,
    constant_props: HashMap<*mut crate::ir::expression::Expression<'a>, ExprRef<'a>>,
}

impl<'a> Visitor<'a> for DfoApplier<'a> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        // Bottom-up visit to handle nested structures correctly
        self.visit_children(expr);

        // 1. Apply Constant Propagation
        if let Some(&const_val) = self.constant_props.get(&expr.as_ptr()) {
            *expr = const_val;
            return;
        }

        // 2. Apply Dead Store Elimination
        if self.dead_definitions.contains(&expr.as_ptr()) {
            // For dead LocalSet/LocalTee with no uses and no side effects inside,
            // we can replace with Nop (since it's in a block or top-level)
            expr.kind = ExpressionKind::Nop;
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::IrBuilder;
    use crate::ir::module::Function;
    use crate::ir::{Literal, Type};
    use bumpalo::Bump;

    #[test]
    fn test_dfo_constant_prop() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // (local.set 0 (i32.const 42))
        // (local.set 1 (local.get 0))
        let c42 = builder.const_(Literal::I32(42));
        let set0 = builder.local_set(0, c42);
        let get0 = builder.local_get(0, Type::I32);
        let set1 = builder.local_set(1, get0);

        let mut list = bumpalo::collections::Vec::new_in(&bump);
        list.push(set0);
        list.push(set1);
        let block = builder.block(None, list, Type::NONE);

        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![Type::I32, Type::I32],
            Some(block),
        );

        let mut module = Module::new(&bump);
        module.add_function(func);

        let mut dfo = DataFlowOpts;
        dfo.run(&mut module);

        // Verify that (local.get 0) was replaced by (i32.const 42)
        let optimized_block = module.functions[0].body.unwrap();
        if let ExpressionKind::Block { list, .. } = &optimized_block.kind {
            if let ExpressionKind::LocalSet { value, .. } = &list[1].kind {
                assert!(matches!(value.kind, ExpressionKind::Const { .. }));
            } else {
                panic!("Expected LocalSet at index 1");
            }
        }
    }
}
