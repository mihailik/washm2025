use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionMarker};
use crate::ir::module::Module;
use crate::ir::visitor::Visitor;
use crate::passes::Pass;

/// Removes all debug information (names, locations).
pub struct StripDebug;

impl Pass for StripDebug {
    fn name(&self) -> &str {
        "StripDebug"
    }

    fn run(&mut self, module: &mut Module) {
        // Clear all debug names
        for (_, func) in &mut module.functions {
            for (_, local) in func.locals_mut().iter_mut() {
                local.name.clear();
            }
        }

        // Clear all annotations (includes locations, local names, etc.)
        module.annotations.clear();
    }
}

/// Removes DWARF debug information only.
pub struct StripDWARF;

impl Pass for StripDWARF {
    fn name(&self) -> &str {
        "StripDWARF"
    }

    fn run(&mut self, module: &mut Module) {
        // Clear only location info from annotations
        module.annotations.clear_locations();
    }
}

/// Propagates debug locations from parent to children.
pub struct PropagateDebugLocs;

impl Pass for PropagateDebugLocs {
    fn name(&self) -> &str {
        "PropagateDebugLocs"
    }

    fn run(&mut self, module: &mut Module) {
        let mut propagator = DebugLocPropagator {
            annotations: &mut module.annotations,
            current_loc: None,
        };

        for (_, func) in &mut module.functions {
            if let Some(mut body) = func.body() {
                propagator.visit(&mut body, &mut module.expressions);
            }
        }
    }
}

struct DebugLocPropagator<'m> {
    annotations: &'m mut crate::ir::annotation::AnnotationStore,
    current_loc: Option<crate::ir::annotation::DebugLocation>,
}

impl Visitor for DebugLocPropagator<'_> {
    fn visit_expression(
        &mut self,
        expr_id: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        let old_loc = self.current_loc;

        // If this expression has a location, it becomes the new current for children
        if let Some(loc) = self.annotations.get_location(*expr_id) {
            self.current_loc = Some(loc);
        } else if let Some(loc) = self.current_loc {
            // Otherwise, inherit parent's location
            self.annotations.set_location(*expr_id, loc);
        }

        self.visit_children(expr_id, arena);
        self.current_loc = old_loc;
    }
}
