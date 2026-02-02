use crate::analysis::call_graph::CallGraph;
use crate::analysis::liveness::Liveness;
use crate::ir::expression::{ExprRef, ExpressionKind, IrBuilder};
use crate::ir::module::{Function, Global, GlobalKind, Memory, MemoryKind, MemoryLimits, Module};
use crate::ir::visitor::{ReadOnlyVisitor, Visitor};
use crate::ir::{FuncId, Literal, Type};
use crate::passes::flatten::Flatten;
use crate::passes::re_finalize::ReFinalize;
use crate::passes::Pass;
use bumpalo::collections::Vec as BumpVec;
use std::collections::{HashMap, HashSet};

pub const ASYNCIFY_STATE: &str = "__asyncify_state";
pub const ASYNCIFY_DATA: &str = "__asyncify_data";
pub const ASYNCIFY_UNWIND: &str = "asyncify_unwind";
pub const ASYNCIFY_CHECK_CALL_INDEX: &str = "asyncify_check_call_index";
pub const ASYNCIFY_GET_CALL_INDEX: &str = "asyncify_get_call_index";
pub const ASYNCIFY_START_UNWIND: &str = "asyncify_start_unwind";
pub const ASYNCIFY_STOP_UNWIND: &str = "asyncify_stop_unwind";
pub const ASYNCIFY_START_REWIND: &str = "asyncify_start_rewind";
pub const ASYNCIFY_STOP_REWIND: &str = "asyncify_stop_rewind";
pub const ASYNCIFY_GET_STATE: &str = "asyncify_get_state";
pub const ASYNCIFY_GET_REWIND_INDEX: &str = "asyncify_get_rewind_index";

#[derive(Clone, Debug, Copy, PartialEq, Eq)]
pub enum AsyncifyState {
    Normal = 0,
    Unwinding = 1,
    Rewinding = 2,
}

#[derive(Clone, Debug)]
pub struct AsyncifyConfig {
    pub imports: Vec<String>,
    pub ignore_imports: bool,
    pub ignore_indirect: bool,
    pub add_list: Vec<String>,
    pub remove_list: Vec<String>,
    pub only_list: Vec<String>,
    pub propagate_add_list: bool,
    pub verbose: bool,
    pub import_globals: bool,
    pub export_globals: bool,
    pub optimize: bool,
}

impl Default for AsyncifyConfig {
    fn default() -> Self {
        Self {
            imports: Vec::new(),
            ignore_imports: false,
            ignore_indirect: false,
            add_list: Vec::new(),
            remove_list: Vec::new(),
            only_list: Vec::new(),
            propagate_add_list: false,
            verbose: false,
            import_globals: false,
            export_globals: false,
            optimize: true,
        }
    }
}

pub struct Asyncify {
    pub config: AsyncifyConfig,
}

impl Asyncify {
    pub fn new(config: AsyncifyConfig) -> Self {
        Self { config }
    }

    fn add_globals<'a>(&self, module: &mut Module<'a>, pointer_type: Type) {
        if module.globals.iter().any(|(_, g)| g.name == ASYNCIFY_STATE) {
            return;
        }

        let builder = IrBuilder::new(module.allocator);

        module.add_global(Global {
            name: ASYNCIFY_STATE.to_string(),
            type_: Type::I32,
            mutable: true,
            kind: GlobalKind::Local {
                init: builder.const_(Literal::I32(0)),
            },
        });

        module.add_global(Global {
            name: ASYNCIFY_DATA.to_string(),
            type_: pointer_type,
            mutable: true,
            kind: GlobalKind::Local {
                init: if pointer_type == Type::I64 {
                    builder.const_(Literal::I64(0))
                } else {
                    builder.const_(Literal::I32(0))
                },
            },
        });

        for ty in [Type::I32, Type::I64, Type::F32, Type::F64] {
            module.add_global(Global {
                name: format!("asyncify_fake_global_{}", ty.to_string()),
                type_: ty,
                mutable: true,
                kind: GlobalKind::Local {
                    init: match ty {
                        Type::I32 => builder.const_(Literal::I32(0)),
                        Type::I64 => builder.const_(Literal::I64(0)),
                        Type::F32 => builder.const_(Literal::F32(0.0)),
                        Type::F64 => builder.const_(Literal::F64(0.0)),
                        _ => unreachable!(),
                    },
                },
            });
        }
    }

    fn add_functions<'a>(&self, module: &mut Module<'a>, pointer_type: Type) {
        let (state_id, _) = module
            .get_global_by_name(ASYNCIFY_STATE)
            .expect("Asyncify state global not found");
        let (data_id, _) = module
            .get_global_by_name(ASYNCIFY_DATA)
            .expect("Asyncify data global not found");

        self.make_helper(
            module,
            ASYNCIFY_START_UNWIND,
            true,
            AsyncifyState::Unwinding,
            pointer_type,
            state_id,
            data_id,
        );
        self.make_helper(
            module,
            ASYNCIFY_STOP_UNWIND,
            false,
            AsyncifyState::Normal,
            pointer_type,
            state_id,
            data_id,
        );
        self.make_helper(
            module,
            ASYNCIFY_START_REWIND,
            true,
            AsyncifyState::Rewinding,
            pointer_type,
            state_id,
            data_id,
        );
        self.make_helper(
            module,
            ASYNCIFY_STOP_REWIND,
            false,
            AsyncifyState::Normal,
            pointer_type,
            state_id,
            data_id,
        );

        // asyncify_get_state
        let builder = IrBuilder::new(module.allocator);
        let get_state_body = builder.global_get(state_id, Type::I32);
        let func = Function::new(
            ASYNCIFY_GET_STATE.to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(get_state_body),
        );
        let id = module.add_function(func);
        module.export_function(id, ASYNCIFY_GET_STATE.to_string());

        // Add magic helpers (replaced during instrumentation)
        let builder = IrBuilder::new(module.allocator);
        for &name in &[
            ASYNCIFY_UNWIND,
            ASYNCIFY_CHECK_CALL_INDEX,
            ASYNCIFY_GET_CALL_INDEX,
            ASYNCIFY_GET_REWIND_INDEX,
        ] {
            let params = if name == ASYNCIFY_CHECK_CALL_INDEX {
                Type::I32
            } else {
                Type::NONE
            };
            let results = if name == ASYNCIFY_UNWIND {
                Type::NONE
            } else {
                Type::I32
            };
            let func = Function::new(
                name.to_string(),
                params,
                results,
                vec![],
                Some(builder.unreachable()),
            );
            module.add_function(func);
        }
    }

    fn make_helper<'a>(
        &self,
        module: &mut Module<'a>,
        name: &str,
        set_data: bool,
        state: AsyncifyState,
        pointer_type: Type,
        state_id: crate::ir::GlobalId,
        data_id: crate::ir::GlobalId,
    ) {
        let builder = IrBuilder::new(module.allocator);
        let mut list = BumpVec::new_in(module.allocator);

        list.push(builder.global_set(state_id, builder.const_(Literal::I32(state as i32))));

        let params = if set_data { pointer_type } else { Type::NONE };

        if set_data {
            list.push(builder.global_set(data_id, builder.local_get(0, pointer_type)));
        }

        let body = builder.block(None, list, Type::NONE);
        let func = Function::new(name.to_string(), params, Type::NONE, vec![], Some(body));

        let id = module.add_function(func);
        module.export_function(id, name.to_string());
    }
}

impl Pass for Asyncify {
    fn name(&self) -> &str {
        "asyncify"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        Flatten.run(module);

        let pointer_type = if module
            .features
            .contains(crate::ir::wasm_features::FeatureSet::Memory64)
        {
            Type::I64
        } else {
            Type::I32
        };

        let memory_id = module.memories.iter().next().map(|(id, _)| id);
        let memory_id = if let Some(id) = memory_id {
            id
        } else {
            module.add_memory(Memory {
                name: "asyncify_memory".to_string(),
                limits: MemoryLimits {
                    initial: 1,
                    maximum: None,
                },
                kind: MemoryKind::Local,
            })
        };

        self.add_globals(module, pointer_type);
        self.add_functions(module, pointer_type);

        let state_id = module
            .get_global_by_name(ASYNCIFY_STATE)
            .map(|(id, _)| id)
            .expect("Asyncify state global not found");

        let analyzer = ModuleAnalyzer::analyze(module, &self.config);

        let ids: Vec<_> = module.functions.iter().map(|(id, _)| id).collect();
        for id in ids {
            if !analyzer.needs_instrumentation(&module.functions[id]) {
                continue;
            }

            let (range, added_locals) = {
                let func = &module.functions[id];
                let base_local_index = (func.params.tuple_len() + func.vars().len()) as u32;
                if let Some(body) = func.body() {
                    let mut body_clone = body.clone();
                    let mut flow = AsyncifyFlow::new_inner(
                        module.allocator,
                        module.features,
                        &module.functions,
                        &analyzer,
                        state_id,
                        base_local_index,
                    );
                    let range = flow.process(&mut body_clone);
                    (Some((range.0, range.1, body_clone)), flow.added_locals)
                } else {
                    (None, Vec::new())
                }
            };

            for ty in added_locals {
                module.functions[id].add_var(ty);
            }

            if let Some((start_idx, end_idx, body)) = range {
                let has_body = module.functions[id].body().is_some();
                *module.functions[id].body_mut() = Some(body);
                if has_body {
                    let mut locals = AsyncifyLocals::new(
                        module,
                        &analyzer,
                        id,
                        state_id,
                        memory_id,
                        pointer_type,
                        start_idx,
                        end_idx,
                    );
                    locals.run();
                }
            }
        }

        self.add_functions(module, pointer_type);
    }
}

pub struct ModuleAnalyzer {
    pub can_change_state: HashSet<String>,
    pub is_top_most_runtime: HashSet<String>,
    pub is_bottom_most_runtime: HashSet<String>,
    pub ignore_indirect: bool,
    pub fake_call_globals: HashMap<Type, String>,
}

impl ModuleAnalyzer {
    pub fn analyze(module: &Module, config: &AsyncifyConfig) -> Self {
        let mut can_change_state = HashSet::new();
        let mut is_top_most_runtime = HashSet::new();
        let mut is_bottom_most_runtime = HashSet::new();

        for func in module.functions.values() {
            let mut analyzer = FunctionStateAnalyzer {
                module,
                config,
                can_change_state: false,
                is_top_most_runtime: false,
                is_bottom_most_runtime: false,
            };
            if let Some(body) = func.body() {
                analyzer.visit(body);
            }
            if analyzer.can_change_state {
                can_change_state.insert(func.name.clone());
            }
            if analyzer.is_top_most_runtime {
                is_top_most_runtime.insert(func.name.clone());
            }
            if analyzer.is_bottom_most_runtime {
                is_bottom_most_runtime.insert(func.name.clone());
            }

            // Check if it's an asyncify import
            if let Some(import) = &func.import {
                let mod_name = &import.module;
                let func_name = &import.name;
                let is_asyncify = mod_name == "asyncify";
                let mut changes = false;
                if is_asyncify {
                    match func_name.as_str() {
                        "start_unwind" | "stop_rewind" => {
                            changes = true;
                            is_top_most_runtime.insert(func_name.clone());
                        }
                        "stop_unwind" | "start_rewind" => {
                            is_bottom_most_runtime.insert(func_name.clone());
                        }
                        _ => {}
                    }
                } else if !config.ignore_imports {
                    if config.imports.is_empty()
                        || config
                            .imports
                            .contains(&format!("{}.{}", mod_name, func_name))
                    {
                        changes = true;
                    }
                }
                if changes {
                    can_change_state.insert(func_name.clone());
                }
            }
        }

        let call_graph = CallGraph::build(module);
        let final_can_change_state = call_graph.propagate_back(can_change_state, |name| {
            !config.remove_list.contains(&name.to_string())
                && !is_bottom_most_runtime.contains(name)
        });

        let mut fake_call_globals: HashMap<Type, String> = HashMap::new();
        for ty in [Type::I32, Type::I64, Type::F32, Type::F64] {
            fake_call_globals.insert(ty, format!("asyncify_fake_global_{}", ty.to_string()));
        }

        Self {
            can_change_state: final_can_change_state,
            is_top_most_runtime,
            is_bottom_most_runtime,
            ignore_indirect: config.ignore_indirect,
            fake_call_globals,
        }
    }

    pub fn needs_instrumentation(&self, func: &Function) -> bool {
        self.can_change_state.contains(&func.name) && !self.is_top_most_runtime.contains(&func.name)
    }

    pub fn can_change_state_expr(&self, module: &Module, expr: ExprRef) -> bool {
        self.can_change_state_expr_inner(&module.functions, expr)
    }

    pub fn can_change_state_expr_inner<'b>(
        &self,
        functions: &crate::collections::id_arena::IdArena<
            crate::ir::module::FuncMarker,
            Function<'b>,
        >,
        expr: ExprRef<'b>,
    ) -> bool {
        let mut visitor = CanChangeStateVisitor {
            functions,
            analyzer: self,
            can_change_state: false,
        };
        visitor.visit(expr);
        visitor.can_change_state
    }
}

struct CanChangeStateVisitor<'a, 'b> {
    functions:
        &'a crate::collections::id_arena::IdArena<crate::ir::module::FuncMarker, Function<'b>>,
    analyzer: &'a ModuleAnalyzer,
    can_change_state: bool,
}

impl<'a, 'b> ReadOnlyVisitor<'a> for CanChangeStateVisitor<'a, 'b> {
    fn visit_expression(&mut self, expr: ExprRef<'a>) {
        if self.can_change_state {
            return;
        }
        match &expr.kind {
            ExpressionKind::Call { target, .. } => {
                let func = &self.functions[*target];
                if self.analyzer.can_change_state.contains(&func.name) {
                    self.can_change_state = true;
                }
            }
            ExpressionKind::CallIndirect { .. } if !self.analyzer.ignore_indirect => {
                self.can_change_state = true
            }
            _ => self.visit_children(expr),
        }
    }
}

struct FunctionStateAnalyzer<'a, 'b> {
    module: &'a Module<'b>,
    config: &'a AsyncifyConfig,
    can_change_state: bool,
    is_top_most_runtime: bool,
    is_bottom_most_runtime: bool,
}

impl<'a, 'b> ReadOnlyVisitor<'a> for FunctionStateAnalyzer<'a, 'b> {
    fn visit_expression(&mut self, expr: ExprRef<'a>) {
        if let ExpressionKind::Call { target, .. } = &expr.kind {
            let func = &self.module.functions[*target];
            if let Some(import) = &func.import {
                if import.module == "asyncify" {
                    match import.name.as_str() {
                        "start_unwind" | "stop_rewind" => {
                            self.can_change_state = true;
                            self.is_top_most_runtime = true;
                        }
                        "stop_unwind" | "start_rewind" => {
                            self.is_bottom_most_runtime = true;
                        }
                        _ => {}
                    }
                }
            }
        }
        if matches!(expr.kind, ExpressionKind::CallIndirect { .. }) && !self.config.ignore_indirect
        {
            self.can_change_state = true;
        }
    }
}

pub struct AsyncifyFlow<'a, 'b> {
    functions:
        &'a crate::collections::id_arena::IdArena<crate::ir::module::FuncMarker, Function<'b>>,
    analyzer: &'a ModuleAnalyzer,
    builder: IrBuilder<'b>,
    state_id: crate::ir::GlobalId,
    call_index: u32,
    added_locals: Vec<Type>,
    next_local_index: u32,
    // (call_index) -> Set of live locals at that call site
    pub site_liveness: HashMap<u32, HashSet<u32>>,
}

impl<'a, 'b> AsyncifyFlow<'a, 'b> {
    pub fn new(
        module: &'a Module<'b>,
        analyzer: &'a ModuleAnalyzer,
        state_id: crate::ir::GlobalId,
        base_local_index: u32,
    ) -> Self {
        Self::new_inner(
            module.allocator,
            module.features,
            &module.functions,
            analyzer,
            state_id,
            base_local_index,
        )
    }

    pub fn new_inner(
        allocator: &'b bumpalo::Bump,
        _features: crate::ir::wasm_features::FeatureSet,
        functions: &'a crate::collections::id_arena::IdArena<
            crate::ir::module::FuncMarker,
            Function<'b>,
        >,
        analyzer: &'a ModuleAnalyzer,
        state_id: crate::ir::GlobalId,
        base_local_index: u32,
    ) -> Self {
        Self {
            functions,
            analyzer,
            builder: IrBuilder::new(allocator),
            state_id,
            call_index: 0,
            added_locals: Vec::new(),
            next_local_index: base_local_index,
            site_liveness: HashMap::new(),
        }
    }

    fn get_function_by_name(&self, name: &str) -> Option<(FuncId, &Function<'b>)> {
        for (id, func) in self.functions.iter() {
            if func.name == name {
                return Some((id, func));
            }
        }
        None
    }

    fn add_local(&mut self, ty: Type) -> u32 {
        let idx = self.next_local_index;
        self.next_local_index += 1;
        self.added_locals.push(ty);
        idx
    }

    fn make_state_check(&self, state: AsyncifyState) -> ExprRef<'b> {
        self.builder.binary(
            crate::ir::ops::BinaryOp::EqInt32,
            self.builder.global_get(self.state_id, Type::I32),
            self.builder.const_(Literal::I32(state as i32)),
            Type::I32,
        )
    }

    fn make_range_check(&self, start: u32, end: u32) -> ExprRef<'b> {
        let (rewind_id, _) = self
            .get_function_by_name(ASYNCIFY_GET_REWIND_INDEX)
            .unwrap();
        let get_rewind_index = self.builder.call(
            rewind_id,
            BumpVec::new_in(self.builder.bump),
            Type::I32,
            false,
        );

        self.builder.binary(
            crate::ir::ops::BinaryOp::AndInt32,
            self.builder.binary(
                crate::ir::ops::BinaryOp::GeUInt32,
                get_rewind_index,
                self.builder.const_(Literal::I32(start as i32)),
                Type::I32,
            ),
            self.builder.binary(
                crate::ir::ops::BinaryOp::LtUInt32,
                get_rewind_index,
                self.builder.const_(Literal::I32(end as i32)),
                Type::I32,
            ),
            Type::I32,
        )
    }

    pub fn process(&mut self, expr: &mut ExprRef<'b>) -> (u32, u32) {
        if !self
            .analyzer
            .can_change_state_expr_inner(self.functions, *expr)
        {
            let old = *expr;
            let start = self.call_index;
            *expr = self.builder.if_(
                self.make_state_check(AsyncifyState::Normal),
                old,
                None,
                expr.type_,
            );
            return (start, self.call_index);
        }
        let start_index = self.call_index;
        match &mut expr.kind {
            ExpressionKind::Block { list, .. } => {
                for child in list.iter_mut() {
                    self.process(child);
                }
            }
            ExpressionKind::If {
                condition,
                if_true,
                if_false,
            } => {
                let true_range = self.process(if_true);
                let _false_range = if let Some(f) = if_false {
                    Some(self.process(f))
                } else {
                    None
                };

                let state_rewinding = self.make_state_check(AsyncifyState::Rewinding);

                // cond = (state == rewinding) ? (index in true_range) : cond
                let in_true_range = if true_range.0 < true_range.1 {
                    self.make_range_check(true_range.0, true_range.1)
                } else {
                    self.builder.const_(Literal::I32(0))
                };

                *condition =
                    self.builder
                        .if_(state_rewinding, in_true_range, Some(*condition), Type::I32);
            }
            ExpressionKind::Loop { body, .. } => {
                let range = self.process(body);
                // Loops are tricky. If we are rewinding into a loop, we must enter it if the index is in range.
                let state_normal = self.make_state_check(AsyncifyState::Normal);
                let state_rewinding = self.make_state_check(AsyncifyState::Rewinding);
                let in_range = if range.0 < range.1 {
                    self.make_range_check(range.0, range.1)
                } else {
                    self.builder.const_(Literal::I32(0))
                };

                let old_body = *body;
                *body = self.builder.if_(
                    self.builder.binary(
                        crate::ir::ops::BinaryOp::OrInt32,
                        state_normal,
                        self.builder.binary(
                            crate::ir::ops::BinaryOp::AndInt32,
                            state_rewinding,
                            in_range,
                            Type::I32,
                        ),
                        Type::I32,
                    ),
                    old_body,
                    None,
                    body.type_,
                );
            }
            ExpressionKind::Call { target, .. } => {
                let target_name = &self.functions[*target].name;
                if self.analyzer.can_change_state.contains(target_name) {
                    *expr = self.make_call_support(*expr)
                }
            }
            ExpressionKind::CallIndirect { .. } => {
                if !self.analyzer.ignore_indirect {
                    *expr = self.make_call_support(*expr)
                }
            }
            ExpressionKind::LocalSet { value, .. }
            | ExpressionKind::LocalTee { value, .. }
            | ExpressionKind::Drop { value } => {
                if self
                    .analyzer
                    .can_change_state_expr_inner(self.functions, *value)
                {
                    self.process(value);
                } else {
                    let old = *expr;
                    *expr = self.builder.if_(
                        self.make_state_check(AsyncifyState::Normal),
                        old,
                        None,
                        expr.type_,
                    );
                }
            }
            _ => {
                let old = *expr;
                *expr = self.builder.if_(
                    self.make_state_check(AsyncifyState::Normal),
                    old,
                    None,
                    expr.type_,
                );
            }
        }
        (start_index, self.call_index)
    }

    fn make_call_support(&mut self, expr: ExprRef<'b>) -> ExprRef<'b> {
        let index = self.call_index;
        self.call_index += 1;

        let site_label = self
            .builder
            .bump
            .alloc_str(&format!("asyncify_site_{}", index));

        let mut ops = BumpVec::new_in(self.builder.bump);
        ops.push(self.builder.const_(Literal::I32(index as i32)));

        let (check_call_id, _) = self
            .get_function_by_name(ASYNCIFY_CHECK_CALL_INDEX)
            .expect("Asyncify check_call_index not found");
        let (unwind_id, _) = self
            .get_function_by_name(ASYNCIFY_UNWIND)
            .expect("Asyncify unwind not found");

        let check_index = self.builder.call(check_call_id, ops, Type::I32, false);
        let condition = self.builder.binary(
            crate::ir::ops::BinaryOp::OrInt32,
            self.make_state_check(AsyncifyState::Normal),
            check_index,
            Type::I32,
        );
        let mut u_ops = BumpVec::new_in(self.builder.bump);
        u_ops.push(self.builder.const_(Literal::I32(index as i32)));
        let unwind_check = self.builder.if_(
            self.make_state_check(AsyncifyState::Unwinding),
            self.builder.call(unwind_id, u_ops, Type::NONE, false),
            None,
            Type::NONE,
        );
        let mut list = BumpVec::new_in(self.builder.bump);

        // After the call, if we were rewinding, we are done rewinding this particular stack frame's reach.
        // Once this call returns, we should be in Normal state.
        let stop_rewind_if_needed = self.builder.if_(
            self.make_state_check(AsyncifyState::Rewinding),
            self.builder.global_set(
                self.state_id,
                self.builder
                    .const_(Literal::I32(AsyncifyState::Normal as i32)),
            ),
            None,
            Type::NONE,
        );

        // Handle return value
        let result_type = expr.type_;
        if result_type != Type::NONE && result_type != Type::UNREACHABLE {
            let temp_local = self.add_local(result_type);
            list.push(self.builder.local_set(temp_local, expr));
            list.push(unwind_check);
            list.push(stop_rewind_if_needed);
            list.push(self.builder.local_get(temp_local, result_type));
        } else {
            list.push(expr);
            list.push(unwind_check);
            list.push(stop_rewind_if_needed);
        }

        self.builder.if_(
            condition,
            self.builder.block(Some(site_label), list, result_type),
            None,
            result_type,
        )
    }
}

pub struct AsyncifyLocals<'a, 'b> {
    module: &'a mut Module<'b>,
    analyzer: &'a ModuleAnalyzer,
    func_id: FuncId,
    state_id: crate::ir::GlobalId,
    memory_id: crate::ir::MemoryId,
    pointer_type: Type,
}

impl<'a, 'b> AsyncifyLocals<'a, 'b> {
    pub fn new(
        module: &'a mut Module<'b>,
        analyzer: &'a ModuleAnalyzer,
        func_id: FuncId,
        state_id: crate::ir::GlobalId,
        memory_id: crate::ir::MemoryId,
        pointer_type: Type,
        _start_index: u32,
        _end_index: u32,
    ) -> Self {
        Self {
            module,
            analyzer,
            func_id,
            state_id,
            memory_id,
            pointer_type,
        }
    }

    pub fn run(&mut self) {
        let (asyncify_data_global_id, _) = self
            .module
            .get_global_by_name(ASYNCIFY_DATA)
            .expect("Asyncify data global not found");
        let memory_id = self.memory_id;

        {
            let func = &mut self.module.functions[self.func_id];
            if let Some(body) = func.body_mut() {
                ReFinalize::run(*body);
            } else {
                return;
            }
        }

        let liveness = Liveness::new(&self.module.functions[self.func_id]);
        let mut relevant_live_locals = HashSet::new();

        struct CallLivenessScanner<'a, 'm> {
            relevant_live_locals: &'a mut HashSet<u32>,
            live_now: HashSet<u32>,
            analyzer: &'a ModuleAnalyzer,
            module: &'m Module<'a>,
        }

        impl<'a, 'm, 'b> Visitor<'b> for CallLivenessScanner<'a, 'm> {
            fn visit_expression(&mut self, expr: &mut ExprRef<'b>) {
                match &mut expr.kind {
                    ExpressionKind::LocalSet { index, value }
                    | ExpressionKind::LocalTee { index, value } => {
                        self.visit(value);
                        self.live_now.insert(*index);
                    }
                    ExpressionKind::LocalGet { index } => {
                        self.live_now.insert(*index);
                    }
                    ExpressionKind::Call { target, .. } => {
                        let target_name = &self.module.functions[*target].name;
                        if self.analyzer.can_change_state.contains(target_name) {
                            self.relevant_live_locals.extend(&self.live_now);
                        }
                    }
                    ExpressionKind::CallIndirect { .. } => {
                        if !self.analyzer.ignore_indirect {
                            self.relevant_live_locals.extend(&self.live_now);
                        }
                    }
                    _ => {
                        self.visit_children(expr);
                    }
                }
            }
        }

        for block in &liveness.cfg.blocks {
            let mut scanner = CallLivenessScanner {
                relevant_live_locals: &mut relevant_live_locals,
                live_now: liveness.live_in[&block.id].clone(),
                analyzer: self.analyzer,
                module: self.module,
            };
            for &expr in &block.contents {
                let mut e = expr;
                scanner.visit(&mut e);
            }
        }

        let pointer_type = self.pointer_type;
        // memory_id is already defined at the start of run()

        let mut live_locals_vec: Vec<u32> = relevant_live_locals.into_iter().collect();
        live_locals_vec.sort();

        let builder = IrBuilder::new(self.module.allocator);
        let data_ptr = builder.global_get(asyncify_data_global_id, pointer_type);

        let rewind_index_local;
        let unwind_index_local;
        let new_inner_body;

        {
            let func = &mut self.module.functions[self.func_id];
            rewind_index_local = func.add_var(Type::I32);
            unwind_index_local = func.add_var(Type::I32);
        }

        {
            let transformer = LocalTransformer {
                builder,
                rewind_index_local,
                unwind_index_local,
                _fake_globals: self.analyzer.fake_call_globals.clone(),
                _fake_locals: HashMap::new(),
                module: self.module,
            };
            let func = &self.module.functions[self.func_id];
            let body = func.body().unwrap();
            new_inner_body = transformer.visit(body);
        }

        let func = &mut self.module.functions[self.func_id];
        let mut load_block_list = BumpVec::new_in(self.module.allocator);

        let mut total_locals_size = 0;
        if !live_locals_vec.is_empty() {
            for &local_idx in &live_locals_vec {
                let ty = func.get_local_type(local_idx);
                total_locals_size += match ty {
                    Type::I64 | Type::F64 => 8,
                    _ => 4,
                };
            }

            // Decrement cursor by total_locals_size to point to the start of the locals block
            load_block_list.push(builder.store(
                memory_id,
                4,
                0,
                4,
                data_ptr,
                builder.binary(
                    crate::ir::ops::BinaryOp::SubInt32,
                    builder.load(memory_id, 4, false, 0, 4, data_ptr, Type::I32),
                    builder.const_(Literal::I32(total_locals_size)),
                    Type::I32,
                ),
            ));

            let stack_ptr_temp = func.add_var(self.pointer_type);
            load_block_list.push(builder.local_set(
                stack_ptr_temp,
                builder.load(memory_id, 4, false, 0, 4, data_ptr, Type::I32),
            ));

            let mut offset = 0;
            for &local_idx in &live_locals_vec {
                let ty = func.get_local_type(local_idx);
                let size = match ty {
                    Type::I64 | Type::F64 => 8,
                    _ => 4,
                };

                load_block_list.push(builder.local_set(
                    local_idx,
                    builder.load(
                        memory_id,
                        size,
                        true,
                        offset,
                        4,
                        builder.local_get(stack_ptr_temp, self.pointer_type),
                        ty,
                    ),
                ));
                offset += size as u32;
            }
        }
        let load_block = builder.block(None, load_block_list, Type::NONE);

        // --- Save Logic (Unwind) ---
        let mut save_block_list = BumpVec::new_in(self.module.allocator);
        if !live_locals_vec.is_empty() {
            let stack_ptr_temp = func.add_var(self.pointer_type);
            save_block_list.push(builder.local_set(
                stack_ptr_temp,
                builder.load(memory_id, 4, false, 0, 4, data_ptr, Type::I32),
            ));

            let mut offset = 0;
            for &local_idx in &live_locals_vec {
                let ty = func.get_local_type(local_idx);
                let size = match ty {
                    Type::I64 | Type::F64 => 8,
                    _ => 4,
                };

                save_block_list.push(builder.store(
                    memory_id,
                    size,
                    offset,
                    4,
                    builder.local_get(stack_ptr_temp, self.pointer_type),
                    builder.local_get(local_idx, ty),
                ));
                offset += size as u32;
            }

            // Increment cursor
            save_block_list.push(builder.store(
                memory_id,
                4,
                0,
                4,
                data_ptr,
                builder.binary(
                    crate::ir::ops::BinaryOp::AddInt32,
                    builder.local_get(stack_ptr_temp, self.pointer_type),
                    builder.const_(Literal::I32(total_locals_size)),
                    Type::I32,
                ),
            ));
        }
        let save_block = builder.block(None, save_block_list, Type::NONE);

        // Combine
        let mut list: BumpVec<ExprRef> = BumpVec::new_in(self.module.allocator);
        let rewinding = builder.binary(
            crate::ir::ops::BinaryOp::EqInt32,
            builder.global_get(self.state_id, Type::I32),
            builder.const_(Literal::I32(AsyncifyState::Rewinding as i32)),
            Type::I32,
        );
        let mut rewind_then = BumpVec::new_in(self.module.allocator);

        // Load locals first (they are at the top of the stack in [index, locals] frame)
        rewind_then.push(load_block);

        // Then load rewind_index (it is now at the new top)
        rewind_then.push(builder.store(
            memory_id,
            4,
            0,
            4,
            data_ptr,
            builder.binary(
                crate::ir::ops::BinaryOp::SubInt32,
                builder.load(memory_id, 4, false, 0, 4, data_ptr, Type::I32),
                builder.const_(Literal::I32(4)),
                Type::I32,
            ),
        ));
        rewind_then.push(builder.local_set(
            rewind_index_local,
            builder.load(memory_id, 4, false, 0, 4, data_ptr, Type::I32),
        ));

        list.push(builder.if_(
            rewinding,
            builder.block(None, rewind_then, Type::NONE),
            None,
            Type::NONE,
        ));

        // The UNWIND block will contain the body
        let mut unwind_block_list = BumpVec::new_in(self.module.allocator);
        unwind_block_list.push(new_inner_body);

        list.push(builder.block(Some(ASYNCIFY_UNWIND), unwind_block_list, Type::NONE));

        let unwinding = builder.binary(
            crate::ir::ops::BinaryOp::EqInt32,
            builder.global_get(self.state_id, Type::I32),
            builder.const_(Literal::I32(AsyncifyState::Unwinding as i32)),
            Type::I32,
        );
        let mut unwind_then = BumpVec::new_in(self.module.allocator);

        // --- Combined Overflow Check and Store ---
        let total_frame_size = 4 + total_locals_size;
        let stack_ptr_temp = func.add_var(self.pointer_type);
        unwind_then.push(builder.local_set(
            stack_ptr_temp,
            builder.load(memory_id, 4, false, 0, 4, data_ptr, Type::I32),
        ));
        let limit = builder.load(memory_id, 4, false, 4, 4, data_ptr, Type::I32);
        let next_ptr = builder.binary(
            crate::ir::ops::BinaryOp::AddInt32,
            builder.local_get(stack_ptr_temp, self.pointer_type),
            builder.const_(Literal::I32(total_frame_size)),
            Type::I32,
        );
        unwind_then.push(builder.if_(
            builder.binary(
                crate::ir::ops::BinaryOp::GtUInt32,
                next_ptr,
                limit,
                Type::I32,
            ),
            builder.unreachable(),
            None,
            Type::NONE,
        ));

        // Store the index first ([index, locals] frame)
        unwind_then.push(builder.store(
            memory_id,
            4,
            0,
            4,
            builder.local_get(stack_ptr_temp, self.pointer_type),
            builder.local_get(unwind_index_local, Type::I32),
        ));
        // Increment cursor for locals
        unwind_then.push(builder.store(
            memory_id,
            4,
            0,
            4,
            data_ptr,
            builder.binary(
                crate::ir::ops::BinaryOp::AddInt32,
                builder.local_get(stack_ptr_temp, self.pointer_type),
                builder.const_(Literal::I32(4)),
                Type::I32,
            ),
        ));

        // Then store locals
        unwind_then.push(save_block);

        list.push(builder.if_(
            unwinding,
            builder.block(None, unwind_then, Type::NONE),
            None,
            Type::NONE,
        ));

        *func.body_mut() = Some(builder.block(None, list, Type::NONE));
    }
}

struct LocalTransformer<'a, 'b> {
    builder: IrBuilder<'b>,
    rewind_index_local: u32,
    unwind_index_local: u32,
    _fake_globals: HashMap<Type, String>,
    _fake_locals: HashMap<Type, u32>,
    module: &'a Module<'b>,
}

impl<'a, 'b> LocalTransformer<'a, 'b> {
    fn visit(&self, expr: ExprRef<'b>) -> ExprRef<'b> {
        let type_ = expr.type_;
        match &expr.kind {
            ExpressionKind::Block { name, list } => {
                let mut new_list = BumpVec::new_in(self.builder.bump);
                for &child in list {
                    new_list.push(self.visit(child));
                }
                self.builder.block(name.clone(), new_list, type_)
            }
            ExpressionKind::If {
                condition,
                if_true,
                if_false,
            } => {
                let cond = self.visit(*condition);
                let true_ = self.visit(*if_true);
                let false_ = if_false.map(|f| self.visit(f));
                self.builder.if_(cond, true_, false_, type_)
            }
            ExpressionKind::Loop { name, body } => {
                let body = self.visit(*body);
                self.builder.loop_(name.clone(), body, type_)
            }
            ExpressionKind::Binary { op, left, right } => {
                let l = self.visit(*left);
                let r = self.visit(*right);
                self.builder.binary(*op, l, r, type_)
            }
            ExpressionKind::Unary { op, value } => {
                let v = self.visit(*value);
                self.builder.unary(*op, v, type_)
            }
            ExpressionKind::LocalSet { index, value } => {
                let v = self.visit(*value);
                self.builder.local_set(*index, v)
            }
            ExpressionKind::LocalTee { index, value } => {
                let v = self.visit(*value);
                self.builder.local_tee(*index, v, expr.type_)
            }
            ExpressionKind::GlobalSet { id, value } => {
                let v = self.visit(*value);
                self.builder.global_set(*id, v)
            }
            ExpressionKind::Drop { value } => {
                let v = self.visit(*value);
                self.builder.drop(v)
            }
            ExpressionKind::Return { value } => {
                let v = value.map(|x| self.visit(x));
                self.builder.return_(v)
            }
            ExpressionKind::Call {
                target,
                operands,
                is_return,
            } => {
                let target_name = &self.module.functions[*target].name;
                if target_name == ASYNCIFY_UNWIND {
                    let capture = self.builder.local_set(self.unwind_index_local, operands[0]);
                    let br = self
                        .builder
                        .break_(ASYNCIFY_UNWIND, None, None, Type::UNREACHABLE);
                    let mut v = BumpVec::new_in(self.builder.bump);
                    v.push(capture);
                    v.push(br);
                    self.builder.block(None, v, Type::UNREACHABLE)
                } else if target_name == ASYNCIFY_CHECK_CALL_INDEX {
                    self.builder.binary(
                        crate::ir::ops::BinaryOp::EqInt32,
                        self.builder.local_get(self.rewind_index_local, Type::I32),
                        operands[0],
                        Type::I32,
                    )
                } else if target_name == ASYNCIFY_GET_REWIND_INDEX {
                    self.builder.local_get(self.rewind_index_local, Type::I32)
                } else if target_name == ASYNCIFY_GET_CALL_INDEX {
                    self.builder.const_(Literal::I32(0))
                } else {
                    let mut new_ops = BumpVec::new_in(self.builder.bump);
                    for &op in operands {
                        new_ops.push(self.visit(op));
                    }
                    self.builder.call(*target, new_ops, expr.type_, *is_return)
                }
            }
            _ => expr,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::IrBuilder;
    use crate::ir::module::Module;
    use crate::ir::Type;
    use bumpalo::Bump;

    #[test]
    fn test_asyncify_basic() {
        let allocator = Bump::new();
        let mut module = Module::new(&allocator);
        let builder = IrBuilder::new(&allocator);

        // Define imports that change state
        let sleep_id = module.functions.push(crate::ir::module::Function {
            name: "sleep".to_string(),
            params: Type::NONE,
            results: Type::NONE,
            vars: vec![],
            body: None,
            import: Some(crate::ir::module::FunctionImport {
                module: "env".to_string(),
                name: "sleep".to_string(),
            }),
            type_idx: None,
            local_names: vec![],
        });

        let mut func = crate::ir::module::Function::new(
            "test_func".to_string(),
            Type::NONE,
            Type::NONE,
            Vec::new(),
            None,
        );

        let sleep_call = builder.call(sleep_id, bumpalo::vec![in &allocator], Type::NONE, false);
        *func.body_mut() =
            Some(builder.block(None, bumpalo::vec![in &allocator; sleep_call], Type::NONE));

        module.functions.push(func);

        let mut config = AsyncifyConfig::default();
        config.imports.push("env.sleep".to_string());

        let mut asyncify = Asyncify::new(config);
        asyncify.run(&mut module);

        // Verification: Check if globals were added
        assert!(module.globals.iter().any(|(_, g)| g.name == ASYNCIFY_STATE));
        assert!(module.globals.iter().any(|(_, g)| g.name == ASYNCIFY_DATA));

        // The function body should have been wrapped in a block
        let func_id = module
            .functions
            .iter()
            .find(|(_, f)| f.name == "test_func")
            .map(|(id, _)| id)
            .unwrap();
        let func = &module.functions[func_id];
        let body = func.body().unwrap();
        if let ExpressionKind::Block { list, .. } = &body.kind {
            // Should have a check for rewinding and the original body (wrapped)
            assert!(list.len() >= 2);
        } else {
            panic!("Function body should be a block now");
        }
    }

    #[test]
    fn test_asyncify_structural_if_else() {
        let allocator = Bump::new();
        let mut module = Module::new(&allocator);
        let builder = IrBuilder::new(&allocator);

        let sleep_id = module.functions.push(crate::ir::module::Function {
            name: "sleep".to_string(),
            params: Type::NONE,
            results: Type::NONE,
            vars: vec![],
            body: None,
            import: Some(crate::ir::module::FunctionImport {
                module: "env".to_string(),
                name: "sleep".to_string(),
            }),
            type_idx: None,
            local_names: vec![],
        });

        let mut func = crate::ir::module::Function::new(
            "if_else".to_string(),
            Type::I32,
            Type::NONE,
            Vec::new(),
            None,
        );

        // if (x) { sleep() } else { nop }
        let sleep_call = builder.call(sleep_id, bumpalo::vec![in &allocator], Type::NONE, false);
        let if_expr = builder.if_(
            builder.local_get(0, Type::I32),
            sleep_call,
            Some(builder.nop()),
            Type::NONE,
        );
        *func.body_mut() = Some(if_expr);

        module.functions.push(func);

        let mut config = AsyncifyConfig::default();
        config.imports.push("env.sleep".to_string());

        let mut asyncify = Asyncify::new(config);
        asyncify.run(&mut module);

        // Verify that the 'if' condition has been instrumented
        let func_id = module
            .functions
            .iter()
            .find(|(_, f)| f.name == "if_else")
            .map(|(id, _)| id)
            .unwrap();
        let func = &module.functions[func_id];
        let body = func.body().unwrap();
        // The body is a block: [load locals/index, original_body, save locals/index]
        if let ExpressionKind::Block { list, .. } = &body.kind {
            let inner_block = list[1]; // The block named ASYNCIFY_UNWIND
            if let ExpressionKind::Block {
                list: inner_list, ..
            } = &inner_block.kind
            {
                let if_instr = inner_list[0];
                if let ExpressionKind::If { condition, .. } = &if_instr.kind {
                    // Condition should be: if (rewinding) { range_check } else { original }
                    if let ExpressionKind::If { .. } = &condition.kind {
                        // Success
                    } else {
                        panic!("Condition should be instrumented with an if");
                    }
                }
            }
        }
    }

    #[test]
    fn test_asyncify_abi_frame_layout() {
        let allocator = Bump::new();
        let mut module = Module::new(&allocator);
        let builder = IrBuilder::new(&allocator);

        module.imports.push(crate::ir::module::Import {
            module: "env".to_string(),
            name: "sleep".to_string(),
            kind: crate::ir::module::ImportKind::Function(Type::NONE, Type::NONE),
        });

        let mut func = crate::ir::module::Function::new(
            "test_abi".to_string(),
            Type::NONE,
            Type::NONE,
            vec![], // no params
            None,
        );
        func.add_var(Type::I32); // local 0

        // Body: local.set 0 (10), sleep(), drop(local.get 0)
        let body = builder.block(
            None,
            bumpalo::vec![in &allocator;
                builder.local_set(0, builder.const_(Literal::I32(10))),
                builder.call(FuncId::from_raw_parts(0, 0), bumpalo::vec![in &allocator], Type::NONE, false),
                builder.drop(builder.local_get(0, Type::I32)),
            ],
            Type::NONE,
        );
        *func.body_mut() = Some(body);
        module.functions.push(func);

        let mut config = AsyncifyConfig::default();
        config.imports.push("env.sleep".to_string());
        let mut asyncify = Asyncify::new(config);
        asyncify.run(&mut module);

        let func = &module.functions[module.functions.iter().next().unwrap().0];
        let body = func.body().unwrap();

        // Check save logic (Unwinding)
        if let ExpressionKind::Block { list, .. } = &body.kind {
            // list[2] is the Unwinding 'if'
            let unwind_if = list[2];
            if let ExpressionKind::If { if_true, .. } = &unwind_if.kind {
                if let ExpressionKind::Block {
                    list: save_list, ..
                } = &if_true.kind
                {
                    // save_list should have:
                    // 1. set stack_ptr_temp = load(asyncify_data)
                    // 2. if (overflow) ...
                    // 3. store index (4 bytes)
                    // 4. store data cursor += 4
                    // 5. block (save locals)

                    let store_index = save_list[2];
                    if let ExpressionKind::Store { offset, value, .. } = &store_index.kind {
                        assert_eq!(*offset, 0, "Index must be at offset 0 of frame");
                        // check that value is the unwind_index_local
                        if let ExpressionKind::LocalGet { .. } = &value.kind {
                            // OK
                        } else {
                            panic!("Index store must use local.get of index");
                        }
                    } else {
                        panic!(
                            "Expected store for index at save_list[2], got {:?}",
                            store_index
                        );
                    }

                    let save_locals_block = save_list[4];
                    if let ExpressionKind::Block {
                        list: local_stores, ..
                    } = &save_locals_block.kind
                    {
                        // local_stores[0] is stack_ptr_temp = load(asyncify_data)
                        // local_stores[1] is store(local 0)
                        let store_local = local_stores[1];
                        if let ExpressionKind::Store { offset, .. } = &store_local.kind {
                            assert_eq!(
                                *offset, 0,
                                "First local must be at offset 0 from NEW cursor (after index)"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn test_asyncify_nop_safety() {
        let allocator = Bump::new();
        let mut module = Module::new(&allocator);
        let builder = IrBuilder::new(&allocator);

        // a regular import that is NOT in the async list
        module.imports.push(crate::ir::module::Import {
            module: "env".to_string(),
            name: "regular".to_string(),
            kind: crate::ir::module::ImportKind::Function(Type::NONE, Type::NONE),
        });

        let mut func = crate::ir::module::Function::new(
            "safe_func".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            None,
        );
        // calls regular() - should NOT be instrumented
        *func.body_mut() = Some(builder.call(
            FuncId::from_raw_parts(0, 0),
            bumpalo::vec![in &allocator],
            Type::NONE,
            false,
        ));
        module.functions.push(func);

        let mut config = AsyncifyConfig::default();
        config.imports.push("env.sleep".to_string()); // different import
        let mut asyncify = Asyncify::new(config);
        asyncify.run(&mut module);

        let func = &module.functions[module.functions.iter().next().unwrap().0];
        let body = func.body().unwrap();

        // Should NOT have a block with state machinery
        if let ExpressionKind::Block { list, .. } = &body.kind {
            // If it's a block, it should just be the original call, NOT the 3-element [load, body, save] block
            assert_ne!(
                list.len(),
                3,
                "Function should not be instrumented if it doesn't call async code"
            );
            // In our implementation, we might wrap it in a block of length 1, that's fine.
            // But it shouldn't have ASYNC_STATE checks.
        }
    }
}
