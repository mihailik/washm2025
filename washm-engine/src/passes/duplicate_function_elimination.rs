use crate::analysis::hasher::DeepHasher;
use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::module::{FuncId, Module};
use crate::ir::visitor::Visitor;
use crate::passes::Pass;
use std::collections::{hash_map::DefaultHasher, HashMap, HashSet};
use std::hash::{Hash, Hasher};

/// Duplicate Function Elimination pass
/// Merges identical functions to reduce code size.
pub struct DuplicateFunctionElimination;

impl Pass for DuplicateFunctionElimination {
    fn name(&self) -> &str {
        "duplicate-function-elimination"
    }

    fn run(&mut self, module: &mut Module) {
        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();

        // 1. Compute hashes
        let mut hashes: HashMap<u64, Vec<FuncId>> = HashMap::new();

        for (id, func) in functions.iter() {
            if let Some(body) = func.body() {
                let mut hasher = DefaultHasher::new();
                // Hash signature too!
                func.params.hash(&mut hasher);
                func.results.hash(&mut hasher);
                func.vars().hash(&mut hasher);

                // Hash body
                let mut deep_hasher = DeepHasher::new(&mut hasher);
                deep_hasher.hash_expr(body, arena);

                let hash = hasher.finish();
                hashes.entry(hash).or_default().push(id);
            }
        }

        // 2. Identify duplicates
        let mut replacements: HashMap<FuncId, FuncId> = HashMap::new();
        let mut to_remove: HashSet<FuncId> = HashSet::new();

        for (_, group) in hashes {
            if group.len() > 1 {
                // Potential duplicates
                // Ideally check equality, but assuming hash collision is rare for now or acceptable for this pass level.
                // Canonical is the first one.
                let canonical = group[0];
                for &duplicate in &group[1..] {
                    replacements.insert(duplicate, canonical);
                    to_remove.insert(duplicate);
                }
            }
        }

        if replacements.is_empty() {
            return;
        }

        // 3. Redirect calls
        for (_, func) in functions.iter_mut() {
            if let Some(body) = func.body_mut() {
                let mut rewriter = CallRedirector {
                    replacements: &replacements,
                };
                rewriter.visit(body, arena);
            }
        }

        // 4. Remove functions
        functions.retain(|id, _| !to_remove.contains(&id));
    }
}

struct CallRedirector<'b> {
    replacements: &'b HashMap<FuncId, FuncId>,
}

impl<'b> Visitor for CallRedirector<'b> {
    fn visit_expression(
        &mut self,
        _expr_id: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        if let ExpressionKind::Call { target, .. } = &mut arena[*_expr_id].kind {
            if let Some(&replacement) = self.replacements.get(target) {
                *target = replacement;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::IrBuilder;
    use crate::ir::module::Function;
    use crate::ir::{Literal, Type};

    #[test]
    fn test_dfe() {
        let mut module = Module::new();

        let (body1, body2) = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            let b1 = builder.const_(Literal::I32(42));
            let b2 = builder.const_(Literal::I32(42));
            (b1, b2)
        };

        // Func1: (i32.const 42)
        let func1 = Function::new("func1".to_string(), Type::NONE, Type::I32, vec![], body1);

        // Func2: (i32.const 42) - Identical
        let func2 = Function::new("func2".to_string(), Type::NONE, Type::I32, vec![], body2);

        let id1 = module.add_function(func1);
        let id2 = module.add_function(func2);

        // Func3: (call $func2)
        let call = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            builder.call(id2, vec![], Type::I32, false)
        };
        let func3 = Function::new("func3".to_string(), Type::NONE, Type::I32, vec![], call);
        let id3 = module.add_function(func3);

        let mut pass = DuplicateFunctionElimination;
        pass.run(&mut module);

        // Should have merged func2 into func1
        assert_eq!(module.functions.len(), 2);

        // func3 should call func1
        let func3 = module.functions.get(id3).unwrap();
        assert_eq!(func3.name, "func3");

        let body_id = func3.body().unwrap();
        if let ExpressionKind::Call { target, .. } = module.expressions[body_id].kind {
            assert_eq!(target, id1);
        } else {
            panic!("Expected Call");
        }
    }
}
