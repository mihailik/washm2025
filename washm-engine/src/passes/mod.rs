pub mod asyncify;
pub mod avoid_reinterprets;
pub mod coalesce_locals;
pub mod code_pushing;
pub mod const_hoisting;
pub mod dae;
pub mod dae_optimizing;
pub mod dce;
pub mod dfo;
pub mod duplicate_function_elimination;
pub mod flatten;
pub mod generate_dyncalls;
pub mod global_refining;
pub mod gufa;
pub mod i64_to_i32_lowering;
pub mod inlining;
pub mod instrument_locals;
pub mod legalize_js_interface;
pub mod licm;
pub mod local_cse;
pub mod local_subtyping;
pub mod memory64_lowering;
pub mod memory_optimization;
pub mod merge_blocks;
pub mod merge_locals;
pub mod metadata;
pub mod names;
pub mod optimize_added_constants;
pub mod optimize_added_constants_propagate;
pub mod optimize_casts;
pub mod optimize_for_js;
pub mod optimize_instructions;
pub mod pick_load_signs;
pub mod poppify;
pub mod post_emscripten;
pub mod precompute;
pub mod precompute_propagate;
pub mod print;
pub mod re_finalize;
pub mod remove_imports;
pub mod remove_memory_init;
pub mod remove_unused_brs;
pub mod remove_unused_module_elements;
pub mod remove_unused_names;
pub mod remove_unused_types;
pub mod reorder_functions;
pub mod reorder_globals;
// pub mod reorder_locals;
pub mod reorder_types;
pub mod rereloop;
pub mod rse;
pub mod safe_heap;
pub mod signature_pruning;
pub mod signext_lowering;
pub mod simplify;
pub mod simplify_control_flow;
pub mod simplify_globals;
pub mod simplify_identity;
pub mod simplify_locals;
pub mod specialized_print;
pub mod ssa;
pub mod ssa_nomerge;
pub mod stack_check;
pub mod strip_debug;
pub mod type_refining;
pub mod type_ssa;
pub mod untee;
pub mod vacuum;

use crate::analysis::validation::Validator;
use crate::ir::module::Module;
use std::collections::HashMap;

pub trait Pass {
    fn name(&self) -> &str;
    fn run<'a>(&mut self, module: &mut Module<'a>);
}

#[derive(Debug, Clone)]
pub struct OptimizationOptions {
    pub debug: bool,
    pub validate: bool,
    pub validate_globally: bool,
    pub optimize_level: u32,
    pub shrink_level: u32,
    pub traps_never_happen: bool,
    pub low_memory_unused: bool,
    pub fast_math: bool,
    pub zero_filled_memory: bool,
    pub closed_world: bool,
    pub debug_info: bool,
    pub inlining: InliningOptions,
}

#[derive(Debug, Clone)]
pub struct InliningOptions {
    pub always_inline_max_size: u32,
    pub one_caller_inline_max_size: u32,
    pub default_inline_max_size: u32,
    pub flexible_inline_max_size: u32,
    pub allow_functions_with_loops: bool,
    pub partial_inlining_ifs: u32,
}

impl Default for InliningOptions {
    fn default() -> Self {
        Self {
            always_inline_max_size: 2,
            one_caller_inline_max_size: u32::MAX,
            default_inline_max_size: 15,
            flexible_inline_max_size: 20,
            allow_functions_with_loops: false,
            partial_inlining_ifs: 0,
        }
    }
}

impl Default for OptimizationOptions {
    fn default() -> Self {
        Self {
            debug: false,
            validate: true,
            validate_globally: true,
            optimize_level: 0,
            shrink_level: 0,
            traps_never_happen: false,
            low_memory_unused: false,
            fast_math: false,
            zero_filled_memory: false,
            closed_world: false,
            debug_info: false,
            inlining: InliningOptions::default(),
        }
    }
}

impl OptimizationOptions {
    pub fn o0() -> Self {
        Self {
            optimize_level: 0,
            shrink_level: 0,
            ..Default::default()
        }
    }

    pub fn o1() -> Self {
        Self {
            optimize_level: 1,
            shrink_level: 0,
            ..Default::default()
        }
    }

    pub fn o2() -> Self {
        Self {
            optimize_level: 2,
            shrink_level: 0,
            ..Default::default()
        }
    }

    pub fn o3() -> Self {
        Self {
            optimize_level: 3,
            shrink_level: 0,
            ..Default::default()
        }
    }

    pub fn o4() -> Self {
        Self {
            optimize_level: 4,
            shrink_level: 0,
            ..Default::default()
        }
    }

    pub fn os() -> Self {
        Self {
            optimize_level: 2,
            shrink_level: 1,
            ..Default::default()
        }
    }

    pub fn oz() -> Self {
        Self {
            optimize_level: 2,
            shrink_level: 2,
            ..Default::default()
        }
    }
}

pub struct PassInfo {
    pub name: &'static str,
    pub description: &'static str,
    pub create: fn(&PassRunner) -> Box<dyn Pass>,
}

pub static PASS_REGISTRY: &[PassInfo] = &[
    PassInfo {
        name: "dce",
        description: "removes unreachable code",
        create: |_| Box::new(self::dce::DCE),
    },
    PassInfo {
        name: "vacuum",
        description: "removes obviously unnecessary code",
        create: |_| Box::new(crate::passes::vacuum::Vacuum),
    },
    PassInfo {
        name: "remove-unused-names",
        description: "removes names from locations that are never branched to",
        create: |_| Box::new(crate::passes::remove_unused_names::RemoveUnusedNames),
    },
    PassInfo {
        name: "remove-unused-brs",
        description: "removes breaks from locations that are never branched to",
        create: |_| Box::new(crate::passes::remove_unused_brs::RemoveUnusedBrs),
    },
    PassInfo {
        name: "remove-unused-module-elements",
        description: "removes unused functions, globals, etc.",
        create: |_| {
            Box::new(crate::passes::remove_unused_module_elements::RemoveUnusedModuleElements)
        },
    },
    PassInfo {
        name: "remove-unused-types",
        description: "removes unused types",
        create: |_| Box::new(crate::passes::remove_unused_types::RemoveUnusedTypes),
    },
    PassInfo {
        name: "simplify-locals",
        description: "miscellaneous locals-related optimizations",
        create: |runner| {
            let allow_tee = runner.get_argument("simplify-locals", "allow-tee") != Some("false");
            let allow_structure =
                runner.get_argument("simplify-locals", "allow-structure") != Some("false");
            let allow_nesting =
                runner.get_argument("simplify-locals", "allow-nesting") != Some("false");
            Box::new(
                crate::passes::simplify_locals::SimplifyLocals::with_options(
                    allow_tee,
                    allow_structure,
                    allow_nesting,
                ),
            )
        },
    },
    PassInfo {
        name: "simplify-locals-notee",
        description: "simplify-locals without creating tees",
        create: |_| {
            Box::new(
                crate::passes::simplify_locals::SimplifyLocals::with_options(false, true, true),
            )
        },
    },
    PassInfo {
        name: "simplify-locals-nostructure",
        description: "simplify-locals without using control flow structure",
        create: |_| {
            Box::new(
                crate::passes::simplify_locals::SimplifyLocals::with_options(true, false, true),
            )
        },
    },
    PassInfo {
        name: "simplify-locals-notee-nostructure",
        description: "simplify-locals without creating tees or using structure",
        create: |_| {
            Box::new(
                crate::passes::simplify_locals::SimplifyLocals::with_options(false, false, true),
            )
        },
    },
    PassInfo {
        name: "coalesce-locals",
        description: "attempts to use fewer locals by sharing them",
        create: |_| Box::new(crate::passes::coalesce_locals::CoalesceLocals),
    },
    PassInfo {
        name: "reorder-locals",
        description: "sorts locals by usage",
        // create: |_| Box::new(crate::passes::reorder_locals::ReorderLocals),
        create: |_| Box::new(crate::passes::dce::DCE), // placeholder
    },
    PassInfo {
        name: "reorder-types",
        description: "sorts types by usage",
        create: |_| Box::new(crate::passes::reorder_types::ReorderTypes),
    },
    /*PassInfo {
        name: "reorder-globals",
        description: "sorts globals by usage",
        create: |_| Box::new(crate::passes::reorder_globals::ReorderGlobals),
    },*/
    PassInfo {
        name: "merge-locals",
        description: "merges locals with the same type",
        create: |_| Box::new(crate::passes::merge_locals::MergeLocals),
    },
    PassInfo {
        name: "merge-blocks",
        description: "merges blocks to their parents",
        create: |_| Box::new(crate::passes::merge_blocks::MergeBlocks),
    },
    PassInfo {
        name: "print",
        description: "prints the module in WAT format",
        create: |_| Box::new(crate::passes::print::Print),
    },
    PassInfo {
        name: "precompute",
        description: "computes constant expressions at compile time",
        create: |_| Box::new(crate::passes::precompute::Precompute),
    },
    PassInfo {
        name: "optimize-instructions",
        description: "peephole-style instruction optimizations",
        create: |_| Box::new(crate::passes::optimize_instructions::OptimizeInstructions::new()),
    },
    PassInfo {
        name: "pick-load-signs",
        description: "optimizes signed/unsigned loads",
        create: |_| Box::new(crate::passes::pick_load_signs::PickLoadSigns),
    },
    PassInfo {
        name: "code-pushing",
        description: "pushes code into places where it is only executed if needed",
        create: |_| Box::new(crate::passes::code_pushing::CodePushing),
    },
    PassInfo {
        name: "duplicate-function-elimination",
        description: "removes duplicate functions",
        create: |_| {
            Box::new(crate::passes::duplicate_function_elimination::DuplicateFunctionElimination)
        },
    },
    PassInfo {
        name: "inlining",
        description: "inlines functions",
        create: |runner| {
            Box::new(
                crate::passes::inlining::Inlining::with_optimization_options(
                    runner.options.clone(),
                ),
            )
        },
    },
    PassInfo {
        name: "inlining-optimizing",
        description: "inlines functions and optimizes locals",
        create: |runner| {
            Box::new(crate::passes::inlining::InliningOptimizing::with_options(
                runner.options.clone(),
            ))
        },
    },
    PassInfo {
        name: "dae-optimizing",
        description: "dead argument elimination and related optimizations",
        create: |_| Box::new(crate::passes::dae_optimizing::DaeOptimizing),
    },
    PassInfo {
        name: "local-cse",
        description: "common subexpression elimination for locals",
        create: |_| Box::new(crate::passes::local_cse::LocalCSE),
    },
    PassInfo {
        name: "type-ssa",
        description: "refine types using SSA",
        create: |_| Box::new(crate::passes::type_ssa::TypeSSA),
    },
    PassInfo {
        name: "gufa",
        description: "global unified flow analysis",
        create: |_| Box::new(crate::passes::gufa::GUFA),
    },
    PassInfo {
        name: "asyncify",
        description: "async/await support",
        create: |_| {
            Box::new(crate::passes::asyncify::Asyncify::new(
                crate::passes::asyncify::AsyncifyConfig::default(),
            ))
        },
    },
    PassInfo {
        name: "avoid-reinterprets",
        description: "optimizes reinterpret instructions",
        create: |_| Box::new(crate::passes::avoid_reinterprets::AvoidReinterprets::new()),
    },
    PassInfo {
        name: "const-hoisting",
        description: "hoist constants out of loops",
        create: |_| Box::new(crate::passes::const_hoisting::ConstHoisting),
    },
    PassInfo {
        name: "dae",
        description: "dead argument elimination",
        create: |_| Box::new(crate::passes::dae::DAE),
    },
    PassInfo {
        name: "dfo",
        description: "data flow optimizations",
        create: |_| Box::new(crate::passes::dfo::DataFlowOpts),
    },
    PassInfo {
        name: "flatten",
        description: "flattens out the IR",
        create: |_| Box::new(crate::passes::flatten::Flatten),
    },
    PassInfo {
        name: "generate-dyncalls",
        description: "generate JavaScript-compatible dycalls",
        create: |_| Box::new(crate::passes::generate_dyncalls::GenerateDynCalls),
    },
    PassInfo {
        name: "global-refining",
        description: "refine types of globals",
        create: |_| Box::new(crate::passes::global_refining::GlobalRefining),
    },
    PassInfo {
        name: "i64-to-i32-lowering",
        description: "lowers i64 to i32 for JS compatibility",
        create: |_| Box::new(crate::passes::i64_to_i32_lowering::I64ToI32Lowering),
    },
    PassInfo {
        name: "instrument-locals",
        description: "instrument locals for debugging",
        create: |_| Box::new(crate::passes::instrument_locals::InstrumentLocals),
    },
    PassInfo {
        name: "legalize-js-interface",
        description: "legalizes JS interface",
        create: |_| Box::new(crate::passes::legalize_js_interface::LegalizeJSInterface),
    },
    PassInfo {
        name: "licm",
        description: "loop invariant code motion",
        create: |_| Box::new(crate::passes::licm::LICM),
    },
    PassInfo {
        name: "local-subtyping",
        description: "apply subtyping to locals",
        create: |_| Box::new(crate::passes::local_subtyping::LocalSubtyping),
    },
    PassInfo {
        name: "memory64-lowering",
        description: "lowers memory64 to memory32",
        create: |_| Box::new(crate::passes::memory64_lowering::Memory64Lowering),
    },
    PassInfo {
        name: "memory-optimization",
        description: "optimizes memory accesses",
        create: |_| Box::new(crate::passes::memory_optimization::MemoryOptimization::new()),
    },
    PassInfo {
        name: "minify-names",
        description: "minifies names of functions, etc.",
        create: |_| Box::new(crate::passes::names::MinifyNames),
    },
    PassInfo {
        name: "strip-names",
        description: "removes names of functions, etc.",
        create: |_| Box::new(crate::passes::names::StripNames),
    },
    PassInfo {
        name: "optimize-added-constants",
        description: "optimizes added constants",
        create: |_| Box::new(crate::passes::optimize_added_constants::OptimizeAddedConstants),
    },
    PassInfo {
        name: "optimize-added-constants-propagate",
        description: "propagates optimized added constants",
        create: |_| {
            Box::new(
                crate::passes::optimize_added_constants_propagate::OptimizeAddedConstantsPropagate,
            )
        },
    },
    PassInfo {
        name: "optimize-casts",
        description: "optimizes casts",
        create: |_| Box::new(crate::passes::optimize_casts::OptimizeCasts::new()),
    },
    PassInfo {
        name: "optimize-for-js",
        description: "optimizes for JS environment",
        create: |_| Box::new(crate::passes::optimize_for_js::OptimizeForJS),
    },
    PassInfo {
        name: "poppify",
        description: "poppify code for Binaryen IR",
        create: |_| Box::new(crate::passes::poppify::Poppify),
    },
    PassInfo {
        name: "post-emscripten",
        description: "post-emscripten optimizations",
        create: |_| Box::new(crate::passes::post_emscripten::PostEmscripten),
    },
    PassInfo {
        name: "precompute-propagate",
        description: "propagates precomputed values",
        create: |_| Box::new(crate::passes::precompute_propagate::PrecomputePropagate),
    },
    PassInfo {
        name: "remove-imports",
        description: "removes imports",
        create: |_| Box::new(crate::passes::remove_imports::RemoveImports),
    },
    PassInfo {
        name: "remove-memory-init",
        description: "removes memory initialization",
        create: |_| Box::new(crate::passes::remove_memory_init::RemoveMemoryInit),
    },
    PassInfo {
        name: "rereloop",
        description: "re-runs the relooper",
        create: |_| Box::new(crate::passes::rereloop::Rereloop),
    },
    PassInfo {
        name: "rse",
        description: "redundant set elimination",
        create: |_| Box::new(crate::passes::rse::RSE),
    },
    PassInfo {
        name: "safe-heap",
        description: "instrument heap accesses for safety",
        create: |_| Box::new(crate::passes::safe_heap::SafeHeap),
    },
    PassInfo {
        name: "signature-pruning",
        description: "prune function signatures",
        create: |_| Box::new(crate::passes::signature_pruning::SignaturePruning),
    },
    PassInfo {
        name: "signext-lowering",
        description: "lowers signext instructions",
        create: |_| Box::new(crate::passes::signext_lowering::SignextLowering),
    },
    PassInfo {
        name: "simplify-control-flow",
        description: "simplifies control flow structures",
        create: |_| Box::new(crate::passes::simplify_control_flow::SimplifyControlFlow),
    },
    PassInfo {
        name: "simplify-globals",
        description: "simplifies globals",
        create: |_| Box::new(crate::passes::simplify_globals::SimplifyGlobals),
    },
    PassInfo {
        name: "simplify-identity",
        description: "removes identity operations",
        create: |_| Box::new(crate::passes::simplify_identity::SimplifyIdentity),
    },
    PassInfo {
        name: "ssa",
        description: "transforms to SSA form",
        create: |_| Box::new(crate::passes::ssa::Ssa),
    },
    PassInfo {
        name: "ssa-nomerge",
        description: "transforms to SSA form without merging",
        create: |_| Box::new(crate::passes::ssa_nomerge::SsaNomerge),
    },
    PassInfo {
        name: "stack-check",
        description: "adds stack check instrumentation",
        create: |_| Box::new(crate::passes::stack_check::StackCheck),
    },
    PassInfo {
        name: "type-refining",
        description: "refines types",
        create: |_| Box::new(crate::passes::type_refining::TypeRefining),
    },
    PassInfo {
        name: "untee",
        description: "removes local.tees",
        create: |_| Box::new(crate::passes::untee::Untee),
    },
];

pub struct PassRunner {
    passes: Vec<Box<dyn Pass>>,
    validate_after_pass: bool,
    pub pass_args: HashMap<String, String>,
    pub options: OptimizationOptions,
}

impl Default for PassRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl PassRunner {
    pub fn new() -> Self {
        Self {
            passes: Vec::new(),
            validate_after_pass: false,
            pass_args: HashMap::new(),
            options: OptimizationOptions::default(),
        }
    }

    pub fn with_options(options: OptimizationOptions) -> Self {
        Self {
            passes: Vec::new(),
            validate_after_pass: options.validate_globally,
            pass_args: HashMap::new(),
            options,
        }
    }

    pub fn get_argument(&self, pass_name: &str, key: &str) -> Option<&str> {
        let full_key = format!("{}@{}", pass_name, key);
        self.pass_args.get(&full_key).map(|s| s.as_str())
    }

    pub fn set_validate_globally(&mut self, validate: bool) {
        self.validate_after_pass = validate;
    }

    pub fn add<P: Pass + 'static>(&mut self, pass: P) {
        self.passes.push(Box::new(pass));
    }

    /// The main entry point for -O1, -O2, etc.
    /// Ported from C++ PassRunner::addDefaultOptimizationPasses.
    pub fn add_default_optimization_passes(&mut self, options: &OptimizationOptions) {
        if options.optimize_level == 0 && options.shrink_level == 0 {
            return; // -O0: no optimizations
        }

        // Global pre-passes
        self.add_global_pre_passes(options);

        // Function-level optimizations (the "meat")
        self.add_function_optimization_passes(options);

        // Global post-passes
        self.add_global_post_passes(options);
    }

    fn add_global_pre_passes(&mut self, options: &OptimizationOptions) {
        self.add(crate::passes::duplicate_function_elimination::DuplicateFunctionElimination);
        if options.optimize_level >= 2 {
            self.add(crate::passes::remove_unused_module_elements::RemoveUnusedModuleElements);
        }
    }

    fn add_function_optimization_passes(&mut self, options: &OptimizationOptions) {
        self.add(self::dce::DCE);
        self.add(crate::passes::remove_unused_names::RemoveUnusedNames);
        self.add(crate::passes::remove_unused_brs::RemoveUnusedBrs);
        self.add(crate::passes::optimize_instructions::OptimizeInstructions::new());

        if options.optimize_level >= 2 || options.shrink_level >= 2 {
            self.add(crate::passes::pick_load_signs::PickLoadSigns);
        }

        self.add(crate::passes::precompute::Precompute);

        if options.optimize_level >= 2 || options.shrink_level >= 2 {
            self.add(crate::passes::code_pushing::CodePushing);
        }

        self.add(crate::passes::simplify_locals::SimplifyLocals::with_options(true, false, true));
        self.add(crate::passes::vacuum::Vacuum);

        self.add(crate::passes::coalesce_locals::CoalesceLocals);
        self.add(crate::passes::vacuum::Vacuum);

        self.add(crate::passes::merge_blocks::MergeBlocks);
        self.add(crate::passes::remove_unused_brs::RemoveUnusedBrs);
        self.add(crate::passes::remove_unused_names::RemoveUnusedNames);
        self.add(crate::passes::merge_blocks::MergeBlocks);

        self.add(crate::passes::precompute::Precompute);
        self.add(crate::passes::optimize_instructions::OptimizeInstructions::new());
        self.add(crate::passes::vacuum::Vacuum);
    }

    fn add_global_post_passes(&mut self, options: &OptimizationOptions) {
        if options.optimize_level >= 2 || options.shrink_level >= 1 {
            self.add(crate::passes::dae_optimizing::DaeOptimizing);
        }
        if (options.optimize_level >= 2 && options.shrink_level == 0) || options.shrink_level >= 2 {
            self.add(crate::passes::inlining::InliningOptimizing::with_options(
                options.clone(),
            ));
        } else if options.optimize_level > 0 || options.shrink_level > 0 {
            self.add(crate::passes::inlining::Inlining::with_optimization_options(options.clone()));
        }
        self.add(crate::passes::duplicate_function_elimination::DuplicateFunctionElimination);
    }

    /// Bundle: Standard cleanup sequence (vacuum + name removal + local simplification)
    pub fn add_cleanup_passes(&mut self) {
        self.add(crate::passes::vacuum::Vacuum);
        self.add(crate::passes::remove_unused_names::RemoveUnusedNames);
        self.add(crate::passes::simplify_locals::SimplifyLocals::new());
    }

    /// Bundle: Dead Code Elimination sequence
    pub fn add_dead_code_elimination_passes(&mut self) {
        self.add(self::dce::DCE);
        self.add(crate::passes::remove_unused_module_elements::RemoveUnusedModuleElements);
    }

    /// Bundle: Branch optimization (merge blocks + remove unused branches)
    pub fn add_branch_optimization_passes(&mut self) {
        self.add(crate::passes::merge_blocks::MergeBlocks);
        self.add(crate::passes::remove_unused_brs::RemoveUnusedBrs);
    }

    pub fn get_all_pass_names() -> Vec<&'static str> {
        PASS_REGISTRY.iter().map(|info| info.name).collect()
    }

    pub fn get_pass_description(name: &str) -> Option<&'static str> {
        PASS_REGISTRY
            .iter()
            .find(|info| info.name == name)
            .map(|info| info.description)
    }

    pub fn add_by_name(&mut self, name: &str) -> bool {
        if let Some(info) = PASS_REGISTRY.iter().find(|info| info.name == name) {
            let pass = (info.create)(self);
            self.passes.push(pass);
            return true;
        }
        false
    }

    pub fn run<'a>(&mut self, module: &mut Module<'a>) {
        for pass in &mut self.passes {
            pass.run(module);

            if self.validate_after_pass {
                let validator = Validator::new(module);
                let (valid, errors) = validator.validate();
                if !valid {
                    let err_msg = errors.join("\n");
                    panic!(
                        "Validation failed after pass '{}':\n{}",
                        pass.name(),
                        err_msg
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::module::Function;
    use crate::ir::Type;

    struct MockPass;

    impl Pass for MockPass {
        fn name(&self) -> &str {
            "MockPass"
        }

        fn run<'a>(&mut self, module: &mut Module<'a>) {
            for (_, func) in &mut module.functions {
                func.name.push_str("_visited");
            }
        }
    }

    #[test]
    fn test_pass_runner() {
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);
        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            None,
        ));

        let mut runner = PassRunner::new();
        runner.add(MockPass);
        runner.run(&mut module);

        assert_eq!(
            module.functions[module.functions.iter().next().unwrap().0].name,
            "test_visited"
        );
    }

    #[test]
    fn test_pass_runner_validation_failure() {
        use crate::ir::expression::{ExprRef, Expression, ExpressionKind};
        use crate::ir::Literal;
        use bumpalo::Bump;

        struct BrokenPass;
        impl Pass for BrokenPass {
            fn name(&self) -> &str {
                "BrokenPass"
            }
            fn run<'a>(&mut self, module: &mut Module<'a>) {
                // Break module validity: Change function return type but not body
                let first_id = module.functions.iter().next().unwrap().0;
                if let Some(func) = module.functions.get_mut(first_id) {
                    func.results = Type::F32;
                }
            }
        }

        let bump = Bump::new();
        let body = bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(42)),
            type_: Type::I32,
        });

        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(ExprRef::new(body)),
        );

        let bump_module = bumpalo::Bump::new();
        let mut module = Module::new(&bump_module);
        module.add_function(func);

        let mut runner = PassRunner::new();
        runner.set_validate_globally(true);
        runner.add(BrokenPass);

        // This should panic
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            runner.run(&mut module);
        }));

        assert!(
            result.is_err(),
            "PassRunner should panic on validation error"
        );
    }

    #[test]
    fn test_optimization_options_presets() {
        let o0 = OptimizationOptions::o0();
        assert_eq!(o0.optimize_level, 0);
        assert_eq!(o0.shrink_level, 0);

        let o3 = OptimizationOptions::o3();
        assert_eq!(o3.optimize_level, 3);
        assert_eq!(o3.shrink_level, 0);

        let os = OptimizationOptions::os();
        assert_eq!(os.optimize_level, 2);
        assert_eq!(os.shrink_level, 1);

        let oz = OptimizationOptions::oz();
        assert_eq!(oz.optimize_level, 2);
        assert_eq!(oz.shrink_level, 2);
    }
}
