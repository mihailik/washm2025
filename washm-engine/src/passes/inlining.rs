use crate::analysis::call_graph::CallGraph;
use crate::analysis::cost::{CostEstimator, TrivialInstruction};
use crate::collections::id_arena::IdArena;
use crate::ir::expression::{
    Expression, ExpressionId, ExpressionKind, ExpressionMarker, IrBuilder,
};
use crate::ir::module::{ExportKind, FuncId, Module};
use crate::ir::visitor::Visitor;
use crate::ir::Type;
use crate::passes::{InliningOptions, OptimizationOptions, Pass};
use std::collections::{HashMap, HashSet};

/// Inlining pass: Inline function calls
pub struct Inlining {
    pub options: OptimizationOptions,
}

struct InliningInfo {
    body: ExpressionId,
    param_ids: Vec<crate::ir::LocalId>,
    all_locals: Vec<(crate::ir::LocalId, Type)>,
}

impl Default for Inlining {
    fn default() -> Self {
        Self::new()
    }
}

impl Inlining {
    pub fn new() -> Self {
        Self {
            options: OptimizationOptions::default(),
        }
    }

    pub fn with_options(options: InliningOptions) -> Self {
        let opt = OptimizationOptions {
            inlining: options,
            ..OptimizationOptions::default()
        };
        Self { options: opt }
    }

    pub fn with_optimization_options(options: OptimizationOptions) -> Self {
        Self { options }
    }

    fn worth_full_inlining(
        &self,
        _callee_name: &str,
        num_callers: usize,
        used_globally: bool,
        callee: &crate::ir::module::Function,
        arena: &IdArena<ExpressionMarker, Expression>,
    ) -> bool {
        let cost = CostEstimator::inline_cost(callee, arena);

        if cost.has_try_delegate {
            return false;
        }

        // Always inline small functions
        if cost.instruction_count <= self.options.inlining.always_inline_max_size {
            return true;
        }

        // Inline if only one caller and not too big
        if num_callers == 1
            && !used_globally
            && cost.instruction_count <= self.options.inlining.one_caller_inline_max_size
        {
            return true;
        }

        // If trivial instruction that shrinks, inline it regardless of other costs
        if cost.trivial_instruction == TrivialInstruction::Shrinks {
            return true;
        }

        // If it's too big for any flexible option, don't inline
        if cost.instruction_count > self.options.inlining.flexible_inline_max_size {
            return false;
        }

        // If we are focused on size or not heavily on speed, don't inline if it might grow
        if self.options.shrink_level > 0 || self.options.optimize_level < 3 {
            return false;
        }

        // Trivial but may not shrink: only inline with O3
        if cost.trivial_instruction == TrivialInstruction::MayNotShrink {
            return self.options.optimize_level >= 3;
        }

        // Default heuristic: no calls and (no loops or allow loops)
        cost.call_count == 0
            && (cost.loop_count == 0 || self.options.inlining.allow_functions_with_loops)
    }
}

impl Pass for Inlining {
    fn name(&self) -> &str {
        "inlining"
    }

    fn run(&mut self, module: &mut Module) {
        let call_graph = CallGraph::build(module);

        // Identify functions used globally
        let mut used_globally = HashSet::new();
        for export in &module.exports {
            if let ExportKind::Function(id) = export.kind {
                if module.functions.get(id).is_some() {
                    used_globally.insert(id);
                }
            }
        }
        if let Some(start) = module.start {
            used_globally.insert(start);
        }
        for (_, segment) in &module.elements {
            for &idx in &segment.func_indices {
                used_globally.insert(idx);
            }
        }

        // Store bodies and signatures
        let mut function_info: HashMap<FuncId, InliningInfo> = HashMap::new();

        for (id, func) in &module.functions {
            if let Some(body) = func.body() {
                function_info.insert(
                    id,
                    InliningInfo {
                        body,
                        param_ids: func.param_ids().to_vec(),
                        all_locals: func.locals().iter().map(|(li, l)| (li, l.type_)).collect(),
                    },
                );
            }
        }

        // 1. Identification: Which functions are good candidates (Full Inlining)
        let mut candidates = HashMap::new();
        for (id, func) in module.functions.iter() {
            let num_callers = call_graph.get_callers(id).map(|s| s.len()).unwrap_or(0);
            let is_used_globally = used_globally.contains(&id);
            if self.worth_full_inlining(
                &func.name,
                num_callers,
                is_used_globally,
                func,
                &module.expressions,
            ) {
                candidates.insert(id, true);
            }
        }

        // 2. Application: Traverse all functions and inline calls to candidates
        let ids: Vec<_> = module.functions.iter().map(|(id, _)| id).collect();
        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        for id in ids {
            if functions[id].body().is_none() {
                continue;
            }

            let mut body = functions[id].body().unwrap();

            // We need to pass the function itself to add new variables
            let mut inliner = Inliner {
                function_info: &function_info,
                candidates: &candidates,
                caller_func: &mut functions[id],
                caller_name: id,
                inline_counter: 0,
            };
            inliner.visit(&mut body, arena);

            if let Some(body_mut) = functions[id].body_mut() {
                *body_mut = body;
            }
        }
    }
}

pub struct InliningOptimizing {
    pub options: OptimizationOptions,
}

impl Default for InliningOptimizing {
    fn default() -> Self {
        Self::new()
    }
}

impl InliningOptimizing {
    pub fn new() -> Self {
        Self {
            options: OptimizationOptions::default(),
        }
    }

    pub fn with_options(options: OptimizationOptions) -> Self {
        Self { options }
    }
}

impl Pass for InliningOptimizing {
    fn name(&self) -> &str {
        "inlining-optimizing"
    }

    fn run(&mut self, module: &mut Module) {
        let mut inlining = Inlining::with_optimization_options(self.options.clone());
        inlining.run(module);

        // Run useful passes after inlining to clean up code growth
        let mut runner = crate::passes::PassRunner::with_options(self.options.clone());
        runner.add(crate::passes::vacuum::Vacuum);
        runner.add(crate::passes::remove_unused_names::RemoveUnusedNames);
        runner.add(crate::passes::simplify_locals::SimplifyLocals::new());
        runner.add(crate::passes::dce::DCE);
        runner.add(crate::passes::precompute::Precompute);
        runner.add(crate::passes::optimize_instructions::OptimizeInstructions::new());
        runner.run(module);
    }
}

struct Inliner<'a> {
    function_info: &'a HashMap<FuncId, InliningInfo>,
    candidates: &'a HashMap<FuncId, bool>,
    caller_func: &'a mut crate::ir::module::Function,
    caller_name: FuncId,
    inline_counter: usize,
}

impl<'a> Visitor for Inliner<'a> {
    fn visit_expression(
        &mut self,
        expr: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        // Bottom-up traversal
        self.visit_children(expr, arena);

        if let ExpressionKind::Call {
            target,
            ref operands,
            ..
        } = arena[*expr].kind
        {
            if !self.candidates.contains_key(&target) {
                return;
            }

            // Don't inline recursive calls to avoid infinite expansion
            if target == self.caller_name {
                return;
            }

            // Copy operands and target to avoid borrowing arena
            let operands = operands.clone();

            if let Some(info) = self.function_info.get(&target) {
                self.inline_counter += 1;
                let inline_id = self.inline_counter;

                let mut block_list = Vec::new();
                let mut mapping = HashMap::new();

                // Map callee locals (params + vars) to new locals in caller
                for (callee_local_id, local_type) in &info.all_locals {
                    let new_id = self
                        .caller_func
                        .locals_mut()
                        .alloc(crate::ir::module::Local {
                            name: String::new(),
                            type_: *local_type,
                            h_type: None,
                        });
                    mapping.insert(*callee_local_id, new_id);
                }

                let mut builder = IrBuilder::new(arena);

                // Initialize param-locals with operand values
                for (i, &callee_param_id) in info.param_ids.iter().enumerate() {
                    let caller_local_id = mapping[&callee_param_id];
                    let operand = operands[i];
                    block_list.push(builder.local_set(caller_local_id, operand));
                }

                // Clone body
                let mut cloned_body = builder.deep_clone(info.body);

                // Remap locals and handle Returns in cloned body
                let display_name = format!("inline${}", inline_id);

                // Pre-allocate block ID for returns to target
                let inline_block_id = builder.arena.alloc(Expression {
                    kind: ExpressionKind::Nop,
                    type_: Type::NONE,
                });

                {
                    let mut remapper = InliningRemapper {
                        mapping,
                        return_target: inline_block_id,
                    };
                    remapper.visit(&mut cloned_body, builder.arena);
                }

                block_list.push(cloned_body);

                let result_type = builder.arena[cloned_body].type_;

                builder.arena[inline_block_id].kind = ExpressionKind::Block {
                    name: Some(display_name),
                    list: block_list,
                };
                builder.arena[inline_block_id].type_ = result_type;

                *expr = inline_block_id;
            }
        }
    }
}

struct InliningRemapper {
    mapping: HashMap<crate::ir::LocalId, crate::ir::LocalId>,
    return_target: ExpressionId,
}

impl Visitor for InliningRemapper {
    fn visit_expression(
        &mut self,
        expr: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        match &mut arena[*expr].kind {
            ExpressionKind::LocalGet { id } => {
                if let Some(new_id) = self.mapping.get(id) {
                    *id = *new_id;
                }
            }
            ExpressionKind::LocalSet { id, .. } | ExpressionKind::LocalTee { id, .. } => {
                if let Some(new_id) = self.mapping.get(id) {
                    *id = *new_id;
                }
            }
            ExpressionKind::Return { value } => {
                // Convert Return to Break to the inlined block
                let val = *value;
                let mut builder = IrBuilder::new(arena);
                *expr = builder.break_(self.return_target, None, val, Type::UNREACHABLE);
                return;
            }
            _ => {}
        }

        self.visit_children(expr, arena);
    }
}
