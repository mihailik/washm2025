use crate::ir::expression::{ExprRef, ExpressionKind};
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

impl<'a> Visitor<'a> for LocalAnalyzer {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        if let ExpressionKind::LocalSet { id, value } | ExpressionKind::LocalTee { id, value } =
            &expr.kind
        {
            let entry = self.assigned_types.entry(*id).or_insert(None);
            if let Some(current) = entry {
                *current = Type::get_lub(*current, value.type_);
            } else {
                *entry = Some(value.type_);
            }
        }
        self.visit_children(expr);
    }
}

impl Pass for LocalSubtyping {
    fn name(&self) -> &str {
        "local-subtyping"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        for (_, func) in &mut module.functions {
            if let Some(mut body) = func.body() {
                // 1. Map from local ID to the LUB of types assigned
                let mut analyzer = LocalAnalyzer {
                    assigned_types: HashMap::new(),
                };
                analyzer.visit(&mut body);
                let inferred_types = analyzer.assigned_types;

                // 2. Update vars (excluding parameters)
                let param_set: std::collections::HashSet<crate::ir::LocalId> =
                    func.param_ids.iter().cloned().collect();

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
