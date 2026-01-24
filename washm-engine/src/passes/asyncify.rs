use crate::analysis::call_graph::CallGraph;
use crate::analysis::liveness::Liveness;
use crate::ir::expression::{ExprRef, ExpressionKind, IrBuilder};
use crate::ir::module::{Function, Global, ImportKind, Module};
use crate::ir::visitor::{ReadOnlyVisitor, Visitor};
use crate::ir::{Literal, Type};
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
        if module.globals.iter().any(|g| g.name == ASYNCIFY_STATE) {
            return;
        }

        let builder = IrBuilder::new(module.allocator);

        module.globals.push(Global {
            name: ASYNCIFY_STATE.to_string(),
            type_: Type::I32,
            mutable: true,
            init: builder.const_(Literal::I32(0)),
        });

        module.globals.push(Global {
            name: ASYNCIFY_DATA.to_string(),
            type_: pointer_type,
            mutable: true,
            init: if pointer_type == Type::I64 {
                builder.const_(Literal::I64(0))
            } else {
                builder.const_(Literal::I32(0))
            },
        });

        for ty in [Type::I32, Type::I64, Type::F32, Type::F64] {
            module.globals.push(Global {
                name: format!("asyncify_fake_global_{}", ty.to_string()),
                type_: ty,
                mutable: true,
                init: match ty {
                    Type::I32 => builder.const_(Literal::I32(0)),
                    Type::I64 => builder.const_(Literal::I64(0)),
                    Type::F32 => builder.const_(Literal::F32(0.0)),
                    Type::F64 => builder.const_(Literal::F64(0.0)),
                    _ => unreachable!(),
                },
            });
        }
    }

    fn add_functions<'a>(&self, module: &mut Module<'a>, pointer_type: Type) {
        let state_idx = module
            .globals
            .iter()
            .position(|g| g.name == ASYNCIFY_STATE)
            .unwrap() as u32;
        let data_idx = module
            .globals
            .iter()
            .position(|g| g.name == ASYNCIFY_DATA)
            .unwrap() as u32;

        self.make_helper(
            module,
            ASYNCIFY_START_UNWIND,
            true,
            AsyncifyState::Unwinding,
            pointer_type,
            state_idx,
            data_idx,
        );
        self.make_helper(
            module,
            ASYNCIFY_STOP_UNWIND,
            false,
            AsyncifyState::Normal,
            pointer_type,
            state_idx,
            data_idx,
        );
        self.make_helper(
            module,
            ASYNCIFY_START_REWIND,
            true,
            AsyncifyState::Rewinding,
            pointer_type,
            state_idx,
            data_idx,
        );
        self.make_helper(
            module,
            ASYNCIFY_STOP_REWIND,
            false,
            AsyncifyState::Normal,
            pointer_type,
            state_idx,
            data_idx,
        );

        // asyncify_get_state
        let builder = IrBuilder::new(module.allocator);
        let get_state_body = builder.global_get(state_idx, Type::I32);
        let func = Function::new(
            ASYNCIFY_GET_STATE.to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(get_state_body),
        );
        let num_func_imports = module
            .imports
            .iter()
            .filter(|i| matches!(i.kind, ImportKind::Function(_, _)))
            .count() as u32;
        let func_idx = num_func_imports + module.functions.len() as u32;
        module.functions.push(func);
        module.export_function(func_idx, ASYNCIFY_GET_STATE.to_string());
    }

    fn make_helper<'a>(
        &self,
        module: &mut Module<'a>,
        name: &str,
        set_data: bool,
        state: AsyncifyState,
        pointer_type: Type,
        state_idx: u32,
        data_idx: u32,
    ) {
        let builder = IrBuilder::new(module.allocator);
        let mut list = BumpVec::new_in(module.allocator);

        list.push(builder.global_set(state_idx, builder.const_(Literal::I32(state as i32))));

        let params = if set_data { pointer_type } else { Type::NONE };

        if set_data {
            list.push(builder.global_set(data_idx, builder.local_get(0, pointer_type)));
        }

        let body = builder.block(None, list, Type::NONE);
        let func = Function::new(name.to_string(), params, Type::NONE, vec![], Some(body));

        let num_func_imports = module
            .imports
            .iter()
            .filter(|i| matches!(i.kind, ImportKind::Function(_, _)))
            .count() as u32;
        let func_idx = num_func_imports + module.functions.len() as u32;

        module.functions.push(func);
        module.export_function(func_idx, name.to_string());
    }
}

impl Pass for Asyncify {
    fn name(&self) -> &str {
        "asyncify"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        Flatten.run(module);

        let analyzer = ModuleAnalyzer::analyze(module, &self.config);

        let pointer_type = if module.memory.as_ref().map_or(false, |m| m.initial > 0) {
            Type::I32 // TODO: check if 64-bit
        } else {
            Type::I32
        };
        self.add_globals(module, pointer_type);

        let state_index = module
            .globals
            .iter()
            .position(|g| g.name == ASYNCIFY_STATE)
            .unwrap() as u32;

        for i in 0..module.functions.len() {
            if !analyzer.needs_instrumentation(&module.functions[i]) {
                continue;
            }

            let mut body = module.functions[i].body.unwrap();
            let base_local_index =
                (module.functions[i].params.tuple_len() + module.functions[i].vars.len()) as u32;
            let (start_idx, end_idx, added_locals) = {
                let mut flow = AsyncifyFlow::new(module, &analyzer, state_index, base_local_index);
                let range = flow.process(&mut body);
                (range.0, range.1, flow.added_locals)
            };

            module.functions[i].body = Some(body);
            for ty in added_locals {
                module.functions[i].add_var(ty);
            }

            let mut locals = AsyncifyLocals::new(
                module,
                &analyzer,
                i,
                state_index,
                pointer_type,
                start_idx,
                end_idx,
            );
            locals.run();
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

        for func in &module.functions {
            let mut analyzer = FunctionStateAnalyzer {
                module,
                config,
                can_change_state: false,
                is_top_most_runtime: false,
                is_bottom_most_runtime: false,
            };
            if let Some(body) = &func.body {
                analyzer.visit(*body);
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
        }

        // Check imports
        for import in &module.imports {
            if let ImportKind::Function(_, _) = &import.kind {
                let is_asyncify = import.module == "asyncify";
                let mut changes = false;
                if is_asyncify {
                    match import.name.as_str() {
                        "start_unwind" | "stop_rewind" => {
                            changes = true;
                            is_top_most_runtime.insert(import.name.clone());
                        }
                        "stop_unwind" | "start_rewind" => {
                            is_bottom_most_runtime.insert(import.name.clone());
                        }
                        _ => {}
                    }
                } else if !config.ignore_imports {
                    if config.imports.is_empty()
                        || config
                            .imports
                            .contains(&format!("{}.{}", import.module, import.name))
                    {
                        changes = true;
                    }
                }
                if changes {
                    can_change_state.insert(import.name.clone());
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
        let mut visitor = CanChangeStateVisitor {
            _module: module,
            analyzer: self,
            can_change_state: false,
        };
        visitor.visit(expr);
        visitor.can_change_state
    }
}

struct CanChangeStateVisitor<'a, 'b> {
    _module: &'a Module<'b>,
    analyzer: &'a ModuleAnalyzer,
    can_change_state: bool,
}

impl<'a, 'b> ReadOnlyVisitor<'a> for CanChangeStateVisitor<'a, 'b> {
    fn visit_expression(&mut self, expr: ExprRef<'a>) {
        if self.can_change_state {
            return;
        }
        match &expr.kind {
            ExpressionKind::Call { target, .. }
                if self.analyzer.can_change_state.contains(*target) =>
            {
                self.can_change_state = true
            }
            ExpressionKind::CallIndirect { .. } if !self.analyzer.ignore_indirect => {
                self.can_change_state = true
            }
            _ => {}
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
            for import in &self.module.imports {
                if import.name == *target && import.module == "asyncify" {
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
    module: &'a Module<'b>,
    analyzer: &'a ModuleAnalyzer,
    builder: IrBuilder<'b>,
    state_index: u32,
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
        state_index: u32,
        base_local_index: u32,
    ) -> Self {
        Self {
            module,
            analyzer,
            builder: IrBuilder::new(module.allocator),
            state_index,
            call_index: 0,
            added_locals: Vec::new(),
            next_local_index: base_local_index,
            site_liveness: HashMap::new(),
        }
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
            self.builder.global_get(self.state_index, Type::I32),
            self.builder.const_(Literal::I32(state as i32)),
            Type::I32,
        )
    }

    fn make_range_check(&self, start: u32, end: u32) -> ExprRef<'b> {
        let get_rewind_index = self.builder.call(
            ASYNCIFY_GET_REWIND_INDEX,
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
        if !self.analyzer.can_change_state_expr(self.module, *expr) {
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
                if self.analyzer.can_change_state.contains(&target.to_string()) {
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
                if self.analyzer.can_change_state_expr(self.module, *value) {
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
            .module
            .allocator
            .alloc_str(&format!("asyncify_site_{}", index));

        let mut ops = BumpVec::new_in(self.module.allocator);
        ops.push(self.builder.const_(Literal::I32(index as i32)));
        let check_index = self
            .builder
            .call(ASYNCIFY_CHECK_CALL_INDEX, ops, Type::I32, false);
        let condition = self.builder.binary(
            crate::ir::ops::BinaryOp::OrInt32,
            self.make_state_check(AsyncifyState::Normal),
            check_index,
            Type::I32,
        );
        let mut u_ops = BumpVec::new_in(self.module.allocator);
        u_ops.push(self.builder.const_(Literal::I32(index as i32)));
        let unwind_check = self.builder.if_(
            self.make_state_check(AsyncifyState::Unwinding),
            self.builder.call(ASYNCIFY_UNWIND, u_ops, Type::NONE, false),
            None,
            Type::NONE,
        );
        let mut list = BumpVec::new_in(self.module.allocator);

        // After the call, if we were rewinding, we are done rewinding this particular stack frame's reach.
        // Once this call returns, we should be in Normal state.
        let stop_rewind_if_needed = self.builder.if_(
            self.make_state_check(AsyncifyState::Rewinding),
            self.builder.global_set(
                self.state_index,
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
    func_idx: usize,
    state_index: u32,
    pointer_type: Type,
}

impl<'a, 'b> AsyncifyLocals<'a, 'b> {
    pub fn new(
        module: &'a mut Module<'b>,
        analyzer: &'a ModuleAnalyzer,
        func_idx: usize,
        state_index: u32,
        pointer_type: Type,
        _start_index: u32,
        _end_index: u32,
    ) -> Self {
        Self {
            module,
            analyzer,
            func_idx,
            state_index,
            pointer_type,
        }
    }

    fn map_global(&self, name: &str) -> String {
        name.to_string()
    }

    pub fn run(&mut self) {
        let asyncify_data_global_idx = self
            .module
            .globals
            .iter()
            .position(|g| g.name == self.map_global(ASYNCIFY_DATA))
            .unwrap() as u32;
        let pointer_type = self.pointer_type;

        let func = &mut self.module.functions[self.func_idx];
        if let Some(body) = func.body {
            ReFinalize::run(body);
        } else {
            return;
        }

        let liveness = Liveness::new(&self.module.functions[self.func_idx]);
        let mut relevant_live_locals = HashSet::new();

        struct CallLivenessScanner<'a> {
            relevant_live_locals: &'a mut HashSet<u32>,
            live_now: HashSet<u32>,
            analyzer: &'a ModuleAnalyzer,
        }

        impl<'a, 'b> Visitor<'b> for CallLivenessScanner<'a> {
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
                        if self.analyzer.can_change_state.contains(&target.to_string()) {
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
            };
            for &expr in &block.contents {
                let mut e = expr;
                scanner.visit(&mut e);
            }
        }

        let builder = IrBuilder::new(self.module.allocator);
        let func = &mut self.module.functions[self.func_idx];

        let rewind_index_local = func.add_var(Type::I32);
        let unwind_index_local = func.add_var(Type::I32);

        let mut transformer = LocalTransformer {
            builder,
            rewind_index_local,
            unwind_index_local,
            _fake_globals: self.analyzer.fake_call_globals.clone(),
            _fake_locals: HashMap::new(),
        };
        let body = func.body.unwrap();
        let new_inner_body = transformer.visit(body);

        // --- Rewind Logic (Load) ---
        let mut live_locals_vec: Vec<u32> = relevant_live_locals.into_iter().collect();
        live_locals_vec.sort();

        let data_ptr = builder.global_get(asyncify_data_global_idx, pointer_type);
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
                4,
                0,
                4,
                data_ptr,
                builder.binary(
                    crate::ir::ops::BinaryOp::SubInt32,
                    builder.load(4, false, 0, 4, data_ptr, Type::I32),
                    builder.const_(Literal::I32(total_locals_size)),
                    Type::I32,
                ),
            ));

            let stack_ptr_temp = func.add_var(self.pointer_type);
            load_block_list.push(builder.local_set(
                stack_ptr_temp,
                builder.load(4, false, 0, 4, data_ptr, Type::I32),
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
                builder.load(4, false, 0, 4, data_ptr, Type::I32),
            ));

            let mut offset = 0;
            for &local_idx in &live_locals_vec {
                let ty = func.get_local_type(local_idx);
                let size = match ty {
                    Type::I64 | Type::F64 => 8,
                    _ => 4,
                };

                save_block_list.push(builder.store(
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
            builder.global_get(self.state_index, Type::I32),
            builder.const_(Literal::I32(AsyncifyState::Rewinding as i32)),
            Type::I32,
        );
        let mut rewind_then = BumpVec::new_in(self.module.allocator);

        // Load locals first (they are at the top of the stack in [index, locals] frame)
        rewind_then.push(load_block);

        // Then load rewind_index (it is now at the new top)
        rewind_then.push(builder.store(
            4,
            0,
            4,
            data_ptr,
            builder.binary(
                crate::ir::ops::BinaryOp::SubInt32,
                builder.load(4, false, 0, 4, data_ptr, Type::I32),
                builder.const_(Literal::I32(4)),
                Type::I32,
            ),
        ));
        rewind_then.push(builder.local_set(
            rewind_index_local,
            builder.load(4, false, 0, 4, data_ptr, Type::I32),
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
            builder.global_get(self.state_index, Type::I32),
            builder.const_(Literal::I32(AsyncifyState::Unwinding as i32)),
            Type::I32,
        );
        let mut unwind_then = BumpVec::new_in(self.module.allocator);

        // --- Combined Overflow Check and Store ---
        let total_frame_size = 4 + total_locals_size;
        let stack_ptr_temp = func.add_var(self.pointer_type);
        unwind_then.push(builder.local_set(
            stack_ptr_temp,
            builder.load(4, false, 0, 4, data_ptr, Type::I32),
        ));
        let limit = builder.load(4, false, 4, 4, data_ptr, Type::I32);
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
            4,
            0,
            4,
            builder.local_get(stack_ptr_temp, self.pointer_type),
            builder.local_get(unwind_index_local, Type::I32),
        ));
        // Increment cursor for locals
        unwind_then.push(builder.store(
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

        func.body = Some(builder.block(None, list, Type::NONE));
    }
}

struct LocalTransformer<'b> {
    builder: IrBuilder<'b>,
    rewind_index_local: u32,
    unwind_index_local: u32,
    _fake_globals: HashMap<Type, String>,
    _fake_locals: HashMap<Type, u32>,
}

impl<'b> LocalTransformer<'b> {
    fn visit(&mut self, expr: ExprRef<'b>) -> ExprRef<'b> {
        match &expr.kind {
            ExpressionKind::Block { name, list } => {
                let mut new_list = BumpVec::new_in(self.builder.bump);
                for &child in list {
                    new_list.push(self.visit(child));
                }
                self.builder.block(name.clone(), new_list, expr.type_)
            }
            ExpressionKind::If {
                condition,
                if_true,
                if_false,
            } => {
                let cond = self.visit(*condition);
                let true_ = self.visit(*if_true);
                let false_ = if_false.map(|f| self.visit(f));
                self.builder.if_(cond, true_, false_, expr.type_)
            }
            ExpressionKind::Loop { name, body } => {
                let body = self.visit(*body);
                self.builder.loop_(name.clone(), body, expr.type_)
            }
            ExpressionKind::Binary { op, left, right } => {
                let l = self.visit(*left);
                let r = self.visit(*right);
                self.builder.binary(*op, l, r, expr.type_)
            }
            ExpressionKind::Unary { op, value } => {
                let v = self.visit(*value);
                self.builder.unary(*op, v, expr.type_)
            }
            ExpressionKind::LocalSet { index, value } => {
                let v = self.visit(*value);
                self.builder.local_set(*index, v)
            }
            ExpressionKind::LocalTee { index, value } => {
                let v = self.visit(*value);
                self.builder.local_tee(*index, v, expr.type_)
            }
            ExpressionKind::GlobalSet { index, value } => {
                let v = self.visit(*value);
                self.builder.global_set(*index, v)
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
                if *target == ASYNCIFY_UNWIND {
                    let capture = self.builder.local_set(self.unwind_index_local, operands[0]);
                    let br = self
                        .builder
                        .break_(ASYNCIFY_UNWIND, None, None, Type::UNREACHABLE);
                    let mut v = BumpVec::new_in(self.builder.bump);
                    v.push(capture);
                    v.push(br);
                    self.builder.block(None, v, Type::UNREACHABLE)
                } else if *target == ASYNCIFY_CHECK_CALL_INDEX {
                    self.builder.binary(
                        crate::ir::ops::BinaryOp::EqInt32,
                        self.builder.local_get(self.rewind_index_local, Type::I32),
                        operands[0],
                        Type::I32,
                    )
                } else if *target == ASYNCIFY_GET_REWIND_INDEX {
                    self.builder.local_get(self.rewind_index_local, Type::I32)
                } else if *target == ASYNCIFY_GET_CALL_INDEX {
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
        module.imports.push(crate::ir::module::Import {
            module: "env".to_string(),
            name: "sleep".to_string(),
            kind: crate::ir::module::ImportKind::Function(Type::NONE, Type::NONE),
        });

        let mut func = crate::ir::module::Function::new(
            "test_func".to_string(),
            Type::NONE,
            Type::NONE,
            Vec::new(),
            None,
        );

        let sleep_call = builder.call("sleep", bumpalo::vec![in &allocator], Type::NONE, false);
        func.body = Some(builder.block(None, bumpalo::vec![in &allocator; sleep_call], Type::NONE));

        module.functions.push(func);

        let mut config = AsyncifyConfig::default();
        config.imports.push("env.sleep".to_string());

        let mut asyncify = Asyncify::new(config);
        asyncify.run(&mut module);

        // Verification: Check if globals were added
        assert!(module.globals.iter().any(|g| g.name == ASYNCIFY_STATE));
        assert!(module.globals.iter().any(|g| g.name == ASYNCIFY_DATA));

        // The function body should have been wrapped in a block
        let func = &module.functions[0];
        let body = func.body.unwrap();
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

        module.imports.push(crate::ir::module::Import {
            module: "env".to_string(),
            name: "sleep".to_string(),
            kind: crate::ir::module::ImportKind::Function(Type::NONE, Type::NONE),
        });

        let mut func = crate::ir::module::Function::new(
            "if_else".to_string(),
            Type::I32,
            Type::NONE,
            Vec::new(),
            None,
        );

        // if (x) { sleep() } else { nop }
        let sleep_call = builder.call("sleep", bumpalo::vec![in &allocator], Type::NONE, false);
        let if_expr = builder.if_(
            builder.local_get(0, Type::I32),
            sleep_call,
            Some(builder.nop()),
            Type::NONE,
        );
        func.body = Some(if_expr);

        module.functions.push(func);

        let mut config = AsyncifyConfig::default();
        config.imports.push("env.sleep".to_string());

        let mut asyncify = Asyncify::new(config);
        asyncify.run(&mut module);

        // Verify that the 'if' condition has been instrumented
        let func = &module.functions[0];
        let body = func.body.unwrap();
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
                builder.call("sleep", bumpalo::vec![in &allocator], Type::NONE, false),
                builder.drop(builder.local_get(0, Type::I32)),
            ],
            Type::NONE,
        );
        func.body = Some(body);
        module.functions.push(func);

        let mut config = AsyncifyConfig::default();
        config.imports.push("env.sleep".to_string());
        let mut asyncify = Asyncify::new(config);
        asyncify.run(&mut module);

        let func = &module.functions[0];
        let body = func.body.unwrap();

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
        func.body = Some(builder.call("regular", bumpalo::vec![in &allocator], Type::NONE, false));
        module.functions.push(func);

        let mut config = AsyncifyConfig::default();
        config.imports.push("env.sleep".to_string()); // different import
        let mut asyncify = Asyncify::new(config);
        asyncify.run(&mut module);

        let func = &module.functions[0];
        let body = func.body.unwrap();

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
