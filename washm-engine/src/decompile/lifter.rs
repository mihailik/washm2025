use crate::analysis::call_graph::CallGraph;
use crate::analysis::global_analysis::GlobalAnalysis;
use crate::decompile::passes::{
    ExpressionRecombination, IdentifyBooleans, IdentifyIfElse, IdentifyLoops, IdentifyPointers,
    NameInferencePass,
};
use crate::ir::module::Module;
use crate::passes::{
    dce::DCE, global_refining::GlobalRefining, gufa::GUFA, local_cse::LocalCSE,
    local_subtyping::LocalSubtyping, merge_blocks::MergeBlocks,
    optimize_instructions::OptimizeInstructions, remove_unused_names::RemoveUnusedNames,
    rereloop::Rereloop, simplify_control_flow::SimplifyControlFlow, vacuum::Vacuum, Pass,
};

/// The Lifter is responsible for running passes that "lift" low-level WASM IR
/// into high-level constructs by populating annotations on Expressions.
pub struct Lifter;

impl Default for Lifter {
    fn default() -> Self {
        Self::new()
    }
}

impl Lifter {
    pub fn new() -> Self {
        Self
    }

    pub fn run(&mut self, module: &mut Module) {
        // 0. Preliminary optimizations and inferences
        LocalSubtyping.run(module);
        GlobalRefining.run(module);
        DCE.run(module);
        GUFA.run(module);

        // 1. Restructure control flow for cleaner decompilation
        SimplifyControlFlow.run(module);
        Vacuum.run(module);
        Rereloop.run(module);
        RemoveUnusedNames.run(module);
        MergeBlocks.run(module);
        LocalCSE.run(module);
        OptimizeInstructions::default().run(module);

        // 2. Run global analysis to inform later passes
        let call_graph = CallGraph::build(module);
        let global_analysis = GlobalAnalysis::analyze(module, &call_graph);

        // 3. Identify high-level types and structures
        IdentifyPointers::new().run(module);
        IdentifyBooleans::new().run(module);

        // 4. Identify control flow patterns in the structured IR
        IdentifyLoops::new().run(module);
        IdentifyIfElse::new().run(module);

        // Run cleanup again after structure detection to remove degenerate patterns
        Vacuum.run(module);
        MergeBlocks.run(module);
        SimplifyControlFlow.run(module);

        // 5. Recombine expressions (inlining single-use locals)
        ExpressionRecombination::run(module);

        // 6. Infer variable names
        NameInferencePass::new().run(module);

        // 7. Use global_analysis to annotate constant globals in IR
        annotate_constant_globals(module, &global_analysis);
    }
}

fn annotate_constant_globals(module: &mut Module, analysis: &GlobalAnalysis) {
    let mut constant_globals = std::collections::HashMap::new();
    for (&id, &lit) in &analysis.global_values {
        constant_globals.insert(id, lit);
    }

    if constant_globals.is_empty() {
        return;
    }

    let mut get_nodes = Vec::new();
    for (_, func) in &module.functions {
        if let crate::ir::module::FunctionKind::Local { body, .. } = &func.kind {
            let mut visitor = GlobalGetCollector {
                get_nodes: &mut get_nodes,
                constant_globals: &constant_globals,
            };
            use crate::ir::visitor::ReadOnlyVisitor;
            visitor.visit(*body, &module.expressions);
        }
    }

    for (expr_id, lit) in get_nodes {
        module.annotations.insert(
            expr_id,
            crate::ir::annotation::Annotation::ConstantGlobal(lit),
        );
    }
}

struct GlobalGetCollector<'a> {
    get_nodes: &'a mut Vec<(crate::ir::expression::ExpressionId, crate::ir::Literal)>,
    constant_globals: &'a std::collections::HashMap<crate::ir::GlobalId, crate::ir::Literal>,
}

impl<'a> crate::ir::visitor::ReadOnlyVisitor for GlobalGetCollector<'a> {
    fn visit_expression(
        &mut self,
        expr_id: crate::ir::expression::ExpressionId,
        arena: &crate::collections::id_arena::IdArena<
            crate::ir::expression::ExpressionMarker,
            crate::ir::expression::Expression,
        >,
    ) {
        if let crate::ir::expression::ExpressionKind::GlobalGet { id } = &arena[expr_id].kind {
            if let Some(&lit) = self.constant_globals.get(id) {
                self.get_nodes.push((expr_id, lit));
            }
        }
    }
}
