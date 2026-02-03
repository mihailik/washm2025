use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::module::Module;
use crate::ir::visitor::Visitor;
use crate::ir::Type;
use crate::passes::Pass;
use std::collections::HashMap;

/// Global Refining pass: Refines global types to more specific subtypes
///
/// This pass analyzes global variable usage (assignments) and refines their types.
pub struct GlobalRefining;

impl Pass for GlobalRefining {
    fn name(&self) -> &str {
        "global-refining"
    }

    fn run(&mut self, module: &mut Module) {
        let (arena, _, functions, globals, _, _, _, _, _, _, _, _, _) = module.parts_mut();

        let mut analyzer = GlobalAnalyzer {
            assigned_types: HashMap::new(),
        };

        // Scan all functions
        for (_, func) in functions {
            if let Some(body) = func.body_mut() {
                analyzer.visit(body, arena);
            }
        }

        // Also scan global initializers themselves
        for (id, global) in globals.iter() {
            if let Some(init) = global.kind_to_init() {
                analyzer.note_assignment(id, arena[init].type_);
            }
        }

        let inferred_types = analyzer.assigned_types;

        // Update globals
        for (id, global) in globals.iter_mut() {
            if let Some(Some(inferred)) = inferred_types.get(&id) {
                if *inferred != global.type_ && inferred.is_subtype_of(global.type_) {
                    global.type_ = *inferred;
                }
            }
        }
    }
}

struct GlobalAnalyzer {
    assigned_types: HashMap<crate::ir::GlobalId, Option<Type>>,
}

impl GlobalAnalyzer {
    fn note_assignment(&mut self, id: crate::ir::GlobalId, type_: Type) {
        let entry = self.assigned_types.entry(id).or_insert(None);
        if let Some(val) = entry {
            *val = Type::get_lub(*val, type_);
        } else {
            *entry = Some(type_);
        }
    }
}

impl Visitor for GlobalAnalyzer {
    fn visit(
        &mut self,
        expr: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        let assignment = if let ExpressionKind::GlobalSet { id, value } = &arena[*expr].kind {
            Some((*id, *value))
        } else {
            None
        };

        if let Some((id, value)) = assignment {
            let value_type = arena[value].type_;
            self.note_assignment(id, value_type);
        }
        self.visit_children(expr, arena);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::IrBuilder;
    use crate::ir::module::GlobalKind;
    use crate::ir::module::{Global, Module};
    use crate::ir::{Literal, Type};

    #[test]
    fn test_global_refining_analysis() {
        // Setup a module with one global
        let mut module = Module::new();
        let (arena, _, _, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        let init = IrBuilder::new(arena).const_(Literal::I32(0));

        module.add_global(Global {
            name: "g0".to_string(),
            type_: Type::I32,
            mutable: true,
            kind: GlobalKind::Local { init },
        });

        let mut pass = GlobalRefining;
        pass.run(&mut module);

        // Nothing changes in MVP, but analysis runs
    }
}
