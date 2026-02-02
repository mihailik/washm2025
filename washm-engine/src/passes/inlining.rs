use crate::analysis::call_graph::CallGraph;
use crate::analysis::cost::{CostEstimator, TrivialInstruction};
use crate::ir::expression::{ExprRef, ExpressionKind, IrBuilder};
use crate::ir::module::{ExportKind, FuncId, Module};
use crate::ir::visitor::Visitor;
use crate::ir::Type;
use crate::passes::{InliningOptions, OptimizationOptions, Pass};
use bumpalo::collections::Vec as BumpVec;
use std::collections::{HashMap, HashSet};

/// Inlining pass: Inline function calls
pub struct Inlining {
    pub options: OptimizationOptions,
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
    ) -> bool {
        let cost = CostEstimator::inline_cost(callee);

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

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        let call_graph = CallGraph::build(module);
        let _allocator = module.allocator;

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
        for segment in &module.elements {
            for &idx in &segment.func_indices {
                used_globally.insert(idx);
            }
        }

        // Store bodies and signatures
        let mut function_info: HashMap<FuncId, (ExprRef<'a>, Type, Vec<Type>)> = HashMap::new();

        for (id, func) in &module.functions {
            if let Some(body) = func.body() {
                function_info.insert(id, (body, func.params, func.vars().to_vec()));
            }
        }

        let allocator = module.allocator;

        // 1. Identification: Which functions are good candidates (Full Inlining)
        let mut candidates = HashMap::new();
        for (id, func) in &module.functions {
            let num_callers = call_graph
                .get_callers(&func.name)
                .map(|s| s.len())
                .unwrap_or(0);
            let is_used_globally = used_globally.contains(&id);
            if self.worth_full_inlining(&func.name, num_callers, is_used_globally, func) {
                candidates.insert(id, true);
            }
        }

        // 2. Application: Traverse all functions and inline calls to candidates
        let ids: Vec<_> = module.functions.iter().map(|(id, _)| id).collect();
        for id in ids {
            if module.functions[id].body().is_none() {
                continue;
            }

            let mut body = module.functions[id].body_mut().take().unwrap();
            let mut vars = std::mem::take(module.functions[id].vars_mut());
            let params = module.functions[id].params;
            let _name = module.functions[id].name.clone();

            let mut inliner = Inliner {
                function_info: &function_info,
                candidates: &candidates,
                builder: IrBuilder::new(allocator),
                caller_vars: &mut vars,
                caller_param_count: params.tuple_len(),
                caller_name: id,
                inline_counter: 0,
            };
            inliner.visit(&mut body);

            // Put vars back
            *module.functions[id].vars_mut() = vars;
            *module.functions[id].body_mut() = Some(body);
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

    fn run<'a>(&mut self, module: &mut Module<'a>) {
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

struct Inliner<'a, 'b> {
    function_info: &'b HashMap<FuncId, (ExprRef<'a>, Type, Vec<Type>)>,
    candidates: &'b HashMap<FuncId, bool>,
    builder: IrBuilder<'a>,
    caller_vars: &'b mut Vec<Type>,
    caller_param_count: usize,
    caller_name: FuncId,
    inline_counter: usize,
}

impl<'a, 'b> Visitor<'a> for Inliner<'a, 'b> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        // Bottom-up traversal
        self.visit_children(expr);

        if let ExpressionKind::Call {
            target, operands, ..
        } = &expr.kind
        {
            if !self.candidates.contains_key(target) {
                return;
            }

            // Don't inline recursive calls to avoid infinite expansion
            if *target == self.caller_name {
                return;
            }

            if let Some((callee_body, callee_params, callee_vars)) = self.function_info.get(target)
            {
                self.inline_counter += 1;
                let inline_id = self.inline_counter;

                // Perform full inlining
                let mut block_list = BumpVec::new_in(self.builder.bump);
                let base_local_index = self.caller_param_count + self.caller_vars.len();

                // Parameters become locals
                let param_types: &[Type] = &callee_params.tuple_elements();
                for (i, &param_type) in param_types.iter().enumerate() {
                    self.caller_vars.push(param_type);
                    let operand = operands[i];
                    block_list.push(
                        self.builder
                            .local_set((base_local_index + i) as u32, operand),
                    );
                }

                // Add callee vars (actual locals)
                for &var_type in callee_vars {
                    self.caller_vars.push(var_type);
                }

                // Clone body
                let mut cloned_body = self.builder.deep_clone(*callee_body);

                // Remap locals and handle Returns in cloned body
                let block_name = format!("inline${}", inline_id);
                let allocated_name = self.builder.bump.alloc_str(&block_name);
                let mut remapper = InliningRemapper {
                    offset: base_local_index as u32,
                    return_target: allocated_name,
                    builder: self.builder,
                };
                remapper.visit(&mut cloned_body);

                block_list.push(cloned_body);

                let result_type = cloned_body.type_;
                let block = self
                    .builder
                    .block(Some(allocated_name), block_list, result_type);
                *expr = block;
            }
        }
    }
}

struct InliningRemapper<'a> {
    offset: u32,
    return_target: &'a str,
    builder: IrBuilder<'a>,
}

impl<'a> Visitor<'a> for InliningRemapper<'a> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        match &mut expr.kind {
            ExpressionKind::LocalGet { index } => {
                *index += self.offset;
            }
            ExpressionKind::LocalSet { index, .. } => {
                *index += self.offset;
            }
            ExpressionKind::LocalTee { index, .. } => {
                *index += self.offset;
            }
            ExpressionKind::Return { value } => {
                // Convert Return to Break to the inlined block
                let val = *value;
                *expr = self
                    .builder
                    .break_(self.return_target, None, val, Type::UNREACHABLE);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::ExpressionKind;
    use crate::ir::module::Function;
    use crate::ir::{Literal, Type};
    use bumpalo::Bump;

    #[test]
    fn test_inlining_full_parity() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Callee: (func $callee (param i32) (result i32) (local.get 0))
        let body = builder.local_get(0, Type::I32);
        let callee = Function::new(
            "callee".to_string(),
            Type::I32,
            Type::I32,
            vec![],
            Some(body),
        );

        // Caller: (func $caller (result i32) (call $callee (i32.const 42)))
        let arg = builder.const_(Literal::I32(42));
        let mut args = BumpVec::new_in(&bump);
        args.push(arg);
        let call = builder.call(FuncId::from_raw_parts(0, 0), args, Type::I32, false);
        let caller = Function::new(
            "caller".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(call),
        );

        let mut module = Module::new(&bump);
        let _callee_id = module.add_function(callee);
        let caller_id = module.add_function(caller);

        let mut inlining = Inlining::new();
        inlining.run(&mut module);

        let caller_body = module.functions[caller_id].body().unwrap();

        // Expect:
        // (block
        //   (local.set 0 (i32.const 42))
        //   (local.get 0)
        // )
        if let ExpressionKind::Block { name, list, .. } = &caller_body.kind {
            assert_eq!(name.as_deref(), Some("inline$1"));
            assert_eq!(list.len(), 2);
            assert!(matches!(
                list[0].kind,
                ExpressionKind::LocalSet { index: 0, .. }
            ));
            assert!(matches!(
                list[1].kind,
                ExpressionKind::LocalGet { index: 0 }
            ));
        } else {
            panic!("Expected Block, got {:?}", caller_body.kind);
        }
    }

    #[test]
    fn test_inlining_return() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);

        // Callee: (func $callee (param i32) (result i32) (return (local.get 0)))
        let body = builder.return_(Some(builder.local_get(0, Type::I32)));
        let callee = Function::new(
            "callee".to_string(),
            Type::I32,
            Type::I32,
            vec![],
            Some(body),
        );

        // Caller: (func $caller (result i32) (call $callee (i32.const 42)))
        let arg = builder.const_(Literal::I32(42));
        let mut args = BumpVec::new_in(&bump);
        args.push(arg);
        let call = builder.call(FuncId::from_raw_parts(0, 0), args, Type::I32, false);
        let caller = Function::new(
            "caller".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(call),
        );

        let mut module = Module::new(&bump);
        module.add_function(callee);
        module.add_function(caller);

        let mut inlining = Inlining::new();
        inlining.run(&mut module);

        let caller_body = module.functions[FuncId::from_raw_parts(1, 0)]
            .body()
            .unwrap();

        // Expect Return to be converted to Break
        if let ExpressionKind::Block { list, .. } = &caller_body.kind {
            if let ExpressionKind::Break { name, value, .. } = &list[1].kind {
                assert_eq!(name, &"inline$1");
                assert!(value.is_some());
            } else {
                panic!("Expected Break, got {:?}", list[1].kind);
            }
        }
    }
}
