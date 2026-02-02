use crate::analysis::cfg::ControlFlowGraph;
use crate::analysis::dominators::DominanceTree;
use crate::analysis::ssa::SSABuilder;
use crate::ir::expression::{ExprRef, ExpressionKind};
use crate::ir::module::Module;
use crate::ir::visitor::Visitor;
use crate::ir::Type;
use crate::passes::Pass;
use std::collections::HashMap;

/// Local Subtyping pass: Refines local types to more specific subtypes using SSA.
pub struct LocalSubtyping;

struct LocalAnalyzer {
    assigned_types: HashMap<u32, Option<Type>>,
}

impl<'a> Visitor<'a> for LocalAnalyzer {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        if let ExpressionKind::LocalSet { index, value }
        | ExpressionKind::LocalTee { index, value } = &expr.kind
        {
            let entry = self.assigned_types.entry(*index).or_insert(None);
            if let Some(current) = entry {
                *current = Type::get_lub(*current, value.type_);
            } else {
                *entry = Some(value.type_);
            }
        }
    }
}

impl Pass for LocalSubtyping {
    fn name(&self) -> &str {
        "local-subtyping"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        for (_, func) in &mut module.functions {
            if let Some(mut body) = func.body() {
                // 1. Build SSA (infrastructure ready)
                let cfg = ControlFlowGraph::build(func, body);
                let dom = DominanceTree::build(&cfg);
                let _ssa = SSABuilder::build(func, &cfg, &dom);

                // 2. Map from original local index to the LUB of types assigned
                let mut analyzer = LocalAnalyzer {
                    assigned_types: HashMap::new(),
                };
                analyzer.visit(&mut body);
                let inferred_types = analyzer.assigned_types;

                let var_start = func.params.tuple_len() as u32;

                // Update vars
                for (i, var_type) in func.vars_mut().iter_mut().enumerate() {
                    let local_index = var_start + i as u32;

                    if let Some(Some(inferred)) = inferred_types.get(&local_index) {
                        if *inferred != *var_type && inferred.is_subtype_of(*var_type) {
                            *var_type = *inferred;
                        }
                    }
                }
            }
        }
    }
}

impl LocalSubtyping {
    fn _note_type(&self, lubs: &mut HashMap<u32, Option<Type>>, index: u32, new_type: Type) {
        let entry = lubs.entry(index).or_insert(None);
        if let Some(current) = entry {
            *current = Type::get_lub(*current, new_type);
        } else {
            *entry = Some(new_type);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::IrBuilder;
    use crate::ir::module::{Function, Module};
    use crate::ir::{Literal, Type};
    use bumpalo::Bump;

    #[test]
    fn test_local_subtyping_noop_mvp() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // (local.set 0 (i32.const 42))
        let c42 = builder.const_(Literal::I32(42));
        let set = builder.local_set(0, c42);

        // Function has local 0 as I32
        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![Type::I32], // Local 0
            Some(set),
        );

        let bump_mod = Bump::new();
        let mut module = Module::new(&bump_mod);
        module.add_function(func);

        let mut pass = LocalSubtyping;
        pass.run(&mut module);

        // Should remain I32
        assert_eq!(
            module.functions[module.functions.iter().next().unwrap().0].vars()[0],
            Type::I32
        );
    }
}
