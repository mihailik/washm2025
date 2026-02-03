use crate::analysis::stats::ModuleStats;
use crate::ir::module::Module;
use crate::passes::Pass;

/// Removes types that are never used in the module.
pub struct RemoveUnusedTypes;

impl Pass for RemoveUnusedTypes {
    fn name(&self) -> &str {
        "RemoveUnusedTypes"
    }

    fn run(&mut self, module: &mut Module) {
        if module.types.is_empty() {
            return;
        }

        let stats = ModuleStats::collect(module);

        // Remove unused types
        module.types.retain(|id, _| {
            stats
                .type_counts
                .get(&(id.index()))
                .map_or(0, |&count| count)
                > 0
        });
    }
}
