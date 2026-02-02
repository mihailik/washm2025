use crate::ir::expression::{ExprRef, ExpressionKind};
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

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        // Map from global index to LUB of assigned types
        // let mut inferred_types: HashMap<u32, Option<Type>> = HashMap::new();

        let mut analyzer = GlobalAnalyzer {
            assigned_types: HashMap::new(),
        };

        // Scan all functions
        for (_, func) in &mut module.functions {
            if let Some(body) = func.body_mut() {
                analyzer.visit(body);
            }
        }

        // Also scan global initializers themselves
        for (id, global) in module.globals.iter() {
            if let Some(init) = global.kind_to_init() {
                analyzer.note_assignment(id.cast(), init.type_);
            }
        }

        let inferred_types = analyzer.assigned_types;

        // Update globals
        for (id, global) in module.globals.iter_mut() {
            if let Some(Some(inferred)) = inferred_types.get(&id.cast()) {
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

impl<'a> Visitor<'a> for GlobalAnalyzer {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        if let ExpressionKind::GlobalSet { id, value } = &mut expr.kind {
            self.note_assignment(*id, value.type_);
            self.visit(value);
        } else {
            self.visit_children(expr);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::{Expression, ExpressionKind};
    use crate::ir::module::GlobalKind;
    use crate::ir::module::{Global, Module};
    use crate::ir::{Literal, Type};
    use bumpalo::Bump;

    #[test]
    fn test_global_refining_analysis() {
        // Setup a module with one global
        let bump = Bump::new();
        let init = bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(0)),
            type_: Type::I32,
        });

        let mut module = Module::new(&bump);
        module.add_global(Global {
            name: "g0".to_string(),
            type_: Type::I32,
            mutable: true,
            kind: GlobalKind::Local {
                init: ExprRef::new(init),
            },
        });

        let mut pass = GlobalRefining;
        pass.run(&mut module);

        // Nothing changes in MVP, but analysis runs
    }
}
