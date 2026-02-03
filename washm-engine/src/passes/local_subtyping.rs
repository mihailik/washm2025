use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::module::Module;
use crate::ir::visitor::Visitor;
use crate::ir::Type;
use crate::passes::Pass;
use std::collections::HashMap;

/// Local Subtyping pass: Refines local types to more specific subtypes using SSA.
pub struct LocalSubtyping;

struct LocalAnalyzer {
    assigned_types: HashMap<crate::ir::LocalId, Option<Type>>,
}

impl Visitor for LocalAnalyzer {
    fn visit_expression(
        &mut self,
        expr: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        let e = &arena[*expr];
        if let ExpressionKind::LocalSet { id, value } | ExpressionKind::LocalTee { id, value } =
            &e.kind
        {
            let value_type = arena[*value].type_;
            let entry = self.assigned_types.entry(*id).or_insert(None);
            if let Some(current) = entry {
                *current = Type::get_lub(*current, value_type);
            } else {
                *entry = Some(value_type);
            }
        }
    }
}

impl Pass for LocalSubtyping {
    fn name(&self) -> &str {
        "local-subtyping"
    }

    fn run(&mut self, module: &mut Module) {
        for (_, func) in &mut module.functions {
            if let Some(mut body) = func.body() {
                // 1. Map from local ID to the LUB of types assigned
                let mut analyzer = LocalAnalyzer {
                    assigned_types: HashMap::new(),
                };
                analyzer.visit(&mut body, &mut module.expressions);
                let inferred_types = analyzer.assigned_types;

                // 2. Update vars (excluding parameters)
                let param_set: std::collections::HashSet<crate::ir::LocalId> =
                    func.param_ids().iter().cloned().collect();

                for (id, local) in func.locals_mut().iter_mut() {
                    let local_id = id.cast();
                    // Check if it's a variable (not a parameter)
                    if param_set.contains(&local_id) {
                        continue;
                    }

                    if let Some(Some(inferred)) = inferred_types.get(&local_id) {
                        if *inferred != local.type_ && inferred.is_subtype_of(local.type_) {
                            local.type_ = *inferred;
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

    #[test]
    fn test_local_subtyping_noop_mvp() {
        let mut module = Module::new();
        let set = {
            let mut builder = IrBuilder::new(&mut module.expressions);

            // (local.set 0 (i32.const 42))
            let c42 = builder.const_(Literal::I32(42));
            builder.local_set(crate::ir::LocalId::from_u32(0), c42)
        };

        // Function has local 0 as I32
        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![Type::I32], // Local 0
            set,
        );

        module.add_function(func);

        let mut pass = LocalSubtyping;
        pass.run(&mut module);

        // Should remain I32
        let func_id = module.functions.iter().next().unwrap().0;
        assert_eq!(module.functions[func_id].vars()[0], Type::I32);
    }
}
