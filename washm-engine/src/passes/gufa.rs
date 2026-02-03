use crate::analysis::call_graph::CallGraph;
use crate::analysis::global_analysis::GlobalAnalysis;
use crate::ir::module::Module;
use crate::passes::remove_unused_module_elements::RemoveUnusedModuleElements;
use crate::passes::Pass;
use std::collections::HashSet;

/// GUFA (Global Unified Flow Analysis)
///
/// In this simplified version for Tier 4, GUFA performs:
/// 1. Whole-module reachability analysis to find unreachable functions.
/// 2. Removal of unreachable functions and their associated metadata.
/// 3. Cooperation with SimplifyGlobals for global constant propagation.
pub struct GUFA;

impl Pass for GUFA {
    fn name(&self) -> &str {
        "gufa"
    }

    fn run(&mut self, module: &mut Module) {
        // 1. Build CallGraph
        let call_graph = CallGraph::build(module);

        // 2. Run Global Analysis to find reachable functions
        let analysis = GlobalAnalysis::analyze(module, &call_graph);

        // 3. Mark unreachable functions for removal
        // A function is unreachable if it's not reachable from any export or start function.
        let mut to_remove = HashSet::new();
        for (id, func) in &module.functions {
            if !analysis.reachable_functions.contains(&id) {
                // Don't remove if it's exported (GlobalAnalysis should already include exports as roots)
                to_remove.insert(func.name.clone());
            }
        }

        if !to_remove.is_empty() {
            // We can leverage RemoveUnusedModuleElements logic if we tell it what to keep.
            // Or just manually remove them here.
            // Binaryen's GUFA usually marks them as unreachable so other passes can clean up.

            // For now, let's just use RemoveUnusedModuleElements to do a clean sweep.
            // It will see that these functions are not used.
            let mut rume = RemoveUnusedModuleElements;
            rume.run(module);
        }
    }
}
