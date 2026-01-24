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
            let mut flow = AsyncifyFlow::new(module, &analyzer, state_index);
            flow.process(&mut body);
            module.functions[i].body = Some(body);

            let mut locals = AsyncifyLocals::new(module, &analyzer, i, state_index, pointer_type);
            locals.run();
        }
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
}

impl<'a, 'b> AsyncifyFlow<'a, 'b> {
    pub fn new(module: &'a Module<'b>, analyzer: &'a ModuleAnalyzer, state_index: u32) -> Self {
        Self {
            module,
            analyzer,
            builder: IrBuilder::new(module.allocator),
            state_index,
            call_index: 0,
        }
    }

    fn make_state_check(&self, state: AsyncifyState) -> ExprRef<'b> {
        self.builder.binary(
            crate::ir::ops::BinaryOp::EqInt32,
            self.builder.global_get(self.state_index, Type::I32),
            self.builder.const_(Literal::I32(state as i32)),
            Type::I32,
        )
    }

    pub fn process(&mut self, expr: &mut ExprRef<'b>) {
        if !self.analyzer.can_change_state_expr(self.module, *expr) {
            let old = *expr;
            *expr = self.builder.if_(
                self.make_state_check(AsyncifyState::Normal),
                old,
                None,
                expr.type_,
            );
            return;
        }
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
                let state_rewinding = self.make_state_check(AsyncifyState::Rewinding);
                *condition = self.builder.binary(
                    crate::ir::ops::BinaryOp::OrInt32,
                    *condition,
                    state_rewinding,
                    Type::I32,
                );
                self.process(if_true);
                if let Some(f) = if_false {
                    self.process(f);
                }
            }
            ExpressionKind::Loop { body, .. } => self.process(body),
            ExpressionKind::Call { .. } | ExpressionKind::CallIndirect { .. } => {
                *expr = self.make_call_support(*expr)
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
    }

    fn make_call_support(&mut self, expr: ExprRef<'b>) -> ExprRef<'b> {
        let index = self.call_index;
        self.call_index += 1;
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
        list.push(expr);
        list.push(unwind_check);
        self.builder.if_(
            condition,
            self.builder.block(None, list, Type::NONE),
            None,
            Type::NONE,
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

    fn make_get_stack_pos(&self, builder: &IrBuilder<'b>) -> ExprRef<'b> {
        let global_idx = self
            .module
            .globals
            .iter()
            .position(|g| g.name == self.map_global(ASYNCIFY_DATA))
            .unwrap() as u32;
        builder.load(
            4,
            false,
            0,
            4,
            builder.global_get(global_idx, self.pointer_type),
            Type::I32,
        )
    }

    fn make_inc_stack_pos(&self, builder: &IrBuilder<'b>, delta: i32) -> ExprRef<'b> {
        let global_idx = self
            .module
            .globals
            .iter()
            .position(|g| g.name == self.map_global(ASYNCIFY_DATA))
            .unwrap() as u32;
        let stack_ptr_addr = builder.global_get(global_idx, self.pointer_type);
        let current = builder.load(4, false, 0, 4, stack_ptr_addr, Type::I32);
        let new_val = builder.binary(
            crate::ir::ops::BinaryOp::AddInt32,
            current,
            builder.const_(Literal::I32(delta)),
            Type::I32,
        );
        builder.store(4, 0, 4, stack_ptr_addr, new_val)
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
        let mut check_call_finder = CheckCallFinder::new();

        for block in &liveness.cfg.blocks {
            let mut found = false;
            for &expr in &block.contents {
                check_call_finder.reset();
                check_call_finder.visit(expr);
                if check_call_finder.found {
                    found = true;
                    break;
                }
            }
            if found {
                if let Some(locals) = liveness.live_in.get(&block.id) {
                    relevant_live_locals.extend(locals);
                }
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
        let new_inner_body = transformer.visit_top_level(body);

        // --- Rewind Logic (Load) ---
        let mut live_locals_vec: Vec<u32> = relevant_live_locals.into_iter().collect();
        live_locals_vec.sort();

        let mut load_block_list = BumpVec::new_in(self.module.allocator);

        if !live_locals_vec.is_empty() {
            let mut total_size = 0;
            for &local_idx in &live_locals_vec {
                let ty = func.get_local_type(local_idx);
                total_size += match ty {
                    Type::I64 | Type::F64 => 8,
                    _ => 4,
                };
            }

            load_block_list.push(builder.store(
                4,
                0,
                4,
                builder.global_get(asyncify_data_global_idx, pointer_type),
                builder.binary(
                    crate::ir::ops::BinaryOp::AddInt32,
                    builder.load(
                        4,
                        false,
                        0,
                        4,
                        builder.global_get(asyncify_data_global_idx, pointer_type),
                        Type::I32,
                    ),
                    builder.const_(Literal::I32(-total_size)),
                    Type::I32,
                ),
            ));
            let stack_ptr_temp = func.add_var(self.pointer_type);
            load_block_list.push(builder.local_set(
                stack_ptr_temp,
                builder.load(
                    4,
                    false,
                    0,
                    4,
                    builder.global_get(asyncify_data_global_idx, pointer_type),
                    Type::I32,
                ),
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
        let mut total_size = 0;
        if !live_locals_vec.is_empty() {
            for &local_idx in &live_locals_vec {
                let ty = func.get_local_type(local_idx);
                total_size += match ty {
                    Type::I64 | Type::F64 => 8,
                    _ => 4,
                };
            }

            let stack_ptr_temp = func.add_var(self.pointer_type);
            save_block_list.push(builder.local_set(
                stack_ptr_temp,
                builder.load(
                    4,
                    false,
                    0,
                    4,
                    builder.global_get(asyncify_data_global_idx, pointer_type),
                    Type::I32,
                ),
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
            save_block_list.push(builder.store(
                4,
                0,
                4,
                builder.global_get(asyncify_data_global_idx, pointer_type),
                builder.binary(
                    crate::ir::ops::BinaryOp::AddInt32,
                    builder.load(
                        4,
                        false,
                        0,
                        4,
                        builder.global_get(asyncify_data_global_idx, pointer_type),
                        Type::I32,
                    ),
                    builder.const_(Literal::I32(total_size)),
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
        rewind_then.push(load_block);
        rewind_then.push(builder.local_set(
            rewind_index_local,
            builder.call(
                ASYNCIFY_GET_CALL_INDEX,
                BumpVec::new_in(self.module.allocator),
                Type::I32,
                false,
            ),
        ));
        list.push(builder.if_(
            rewinding,
            builder.block(None, rewind_then, Type::NONE),
            None,
            Type::NONE,
        ));

        list.push(builder.block(
            Some(ASYNCIFY_UNWIND),
            {
                let mut v = BumpVec::new_in(self.module.allocator);
                v.push(new_inner_body);
                v
            },
            Type::NONE,
        ));

        let unwinding = builder.binary(
            crate::ir::ops::BinaryOp::EqInt32,
            builder.global_get(self.state_index, Type::I32),
            builder.const_(Literal::I32(AsyncifyState::Unwinding as i32)),
            Type::I32,
        );
        let mut unwind_then = BumpVec::new_in(self.module.allocator);
        unwind_then.push(builder.store(
            4,
            0,
            4,
            builder.global_get(asyncify_data_global_idx, pointer_type),
            builder.local_get(unwind_index_local, Type::I32),
        ));
        unwind_then.push(builder.store(
            4,
            0,
            4,
            builder.global_get(asyncify_data_global_idx, pointer_type),
            builder.binary(
                crate::ir::ops::BinaryOp::AddInt32,
                builder.load(
                    4,
                    false,
                    0,
                    4,
                    builder.global_get(asyncify_data_global_idx, pointer_type),
                    Type::I32,
                ),
                builder.const_(Literal::I32(4)),
                Type::I32,
            ),
        ));
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
    fn visit_top_level(&mut self, expr: ExprRef<'b>) -> ExprRef<'b> {
        match &expr.kind {
            ExpressionKind::Block { name, list } => {
                let mut new_list = BumpVec::new_in(self.builder.bump);
                for &child in list {
                    new_list.push(self.visit_top_level(child));
                }
                self.builder.block(name.clone(), new_list, expr.type_)
            }
            ExpressionKind::Call {
                target, operands, ..
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
                } else if *target == ASYNCIFY_GET_CALL_INDEX {
                    self.builder.const_(Literal::I32(0))
                } else {
                    expr
                }
            }
            _ => expr,
        }
    }
}

struct CheckCallFinder {
    found: bool,
}

impl CheckCallFinder {
    fn new() -> Self {
        Self { found: false }
    }
    fn reset(&mut self) {
        self.found = false;
    }
}

impl<'a> ReadOnlyVisitor<'a> for CheckCallFinder {
    fn visit_expression(&mut self, expr: ExprRef<'a>) {
        if self.found {
            return;
        }
        if let ExpressionKind::Call { target, .. } = &expr.kind {
            if *target == ASYNCIFY_CHECK_CALL_INDEX {
                self.found = true;
                return;
            }
        }
        self.visit_children(expr);
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
}
