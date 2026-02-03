use crate::analysis::cfg::ControlFlowGraph;
use crate::analysis::dominators::DominanceTree;
use crate::analysis::effects::EffectAnalyzer;
use crate::analysis::ssa::{DefA, SSABuilder};
use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
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

    fn run(&mut self, module: &mut Module) {
        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        for (_, func) in functions {
            if let Some(body_id) = func.body() {
                // 1. Build CFG
                let cfg = ControlFlowGraph::build(func, body_id, arena);

                // 2. Build Dominator Tree (required for SSA)
                let dom = DominanceTree::build(&cfg);

                // 3. Build SSA info (including Def-Use chains)
                let ssa = SSABuilder::build(func, &cfg, &dom, arena);

                // 4. Optimization Phase: Use SSA to find refinements
                let mut dead_definitions = HashSet::new();
                let mut constant_props: HashMap<ExpressionId, ExpressionId> = HashMap::new();

                // Dead Store Elimination and Constant Propagation
                for (def, uses) in &ssa.def_uses {
                    if let DefA::Instruction(instr_id) = def {
                        let instr_kind = &arena[*instr_id].kind;
                        if let ExpressionKind::LocalSet { value, .. }
                        | ExpressionKind::LocalTee { value, .. } = instr_kind
                        {
                            // Dead Store Elimination: No uses and no side effects
                            if uses.is_empty() {
                                let effects = EffectAnalyzer::analyze(*value, arena);
                                if !effects.has_side_effects() {
                                    dead_definitions.insert(*instr_id);
                                }
                            }

                            // Simple Constant Propagation: If value is a constant, propagate to all uses
                            if let ExpressionKind::Const(_) = &arena[*value].kind {
                                for &use_expr in uses {
                                    constant_props.insert(use_expr, *value);
                                }
                            }

                            // Generalized Copy Propagation: local.set $v2, (local.get $v1)
                            if let ExpressionKind::LocalGet { .. } = &arena[*value].kind {
                                for &use_expr in uses {
                                    constant_props.insert(use_expr, *value);
                                }
                            }
                        }
                    }
                }

                // 5. Apply transformations
                if let Some(body_mut) = func.body_mut() {
                    let mut applier = DfoApplier {
                        dead_definitions,
                        constant_props,
                    };
                    applier.visit(body_mut, arena);
                }
            }
        }
    }
}

struct DfoApplier {
    dead_definitions: HashSet<ExpressionId>,
    constant_props: HashMap<ExpressionId, ExpressionId>,
}

impl Visitor for DfoApplier {
    fn visit(
        &mut self,
        expr_id: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        // Bottom-up visit to handle nested structures correctly
        self.visit_children(expr_id, arena);

        // 1. Apply Constant Propagation
        if let Some(&const_val) = self.constant_props.get(expr_id) {
            *expr_id = const_val;
            return;
        }

        // 2. Apply Dead Store Elimination
        if self.dead_definitions.contains(expr_id) {
            arena[*expr_id].kind = ExpressionKind::Nop;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::IrBuilder;
    use crate::ir::module::Function;
    use crate::ir::{Literal, LocalId, Type};

    #[test]
    fn test_dfo_constant_prop() {
        let mut module = Module::new();

        let block = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            // (local.set 0 (i32.const 42))
            // (local.set 1 (local.get 0))
            let c42 = builder.const_(Literal::I32(42));
            let set0 = builder.local_set(LocalId::from(0), c42);
            let get0 = builder.local_get(LocalId::from(0), Type::I32);
            let set1 = builder.local_set(LocalId::from(1), get0);

            builder.block(None::<String>, vec![set0, set1], Type::NONE)
        };

        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![Type::I32, Type::I32],
            block,
        );

        module.add_function(func);

        let mut dfo = DataFlowOpts;
        dfo.run(&mut module);

        // Verify that (local.get 0) was replaced by (i32.const 42)
        let func_id = module.functions.iter().next().unwrap().0;
        let optimized_block_id = module.functions[func_id].body().unwrap();
        if let ExpressionKind::Block { list, .. } = &module.expressions[optimized_block_id].kind {
            if let ExpressionKind::LocalSet { value, .. } = &module.expressions[list[1]].kind {
                assert!(matches!(
                    module.expressions[*value].kind,
                    ExpressionKind::Const(_)
                ));
            } else {
                panic!("Expected LocalSet at index 1");
            }
        }
    }
}
