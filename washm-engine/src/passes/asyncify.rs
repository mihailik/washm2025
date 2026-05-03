use std::collections::{HashMap, HashSet};

use crate::analysis::call_graph::CallGraph;
use crate::analysis::liveness::Liveness;
use crate::collections::id_arena::IdArena;
use crate::ir::annotation::AnnotationStore;
use crate::ir::expression::{
    Expression, ExpressionId, ExpressionKind, ExpressionMarker, IrBuilder,
};
use crate::ir::module::{
    FuncId, FuncMarker, Function, FunctionKind, Global, GlobalKind, LocalId, Memory, MemoryKind,
    MemoryLimits, Module,
};
use crate::ir::visitor::ReadOnlyVisitor;
use crate::ir::Literal;
use crate::ir::Type;
use crate::passes::flatten::Flatten;
use crate::passes::re_finalize::ReFinalize;
use crate::passes::Pass;

pub const ASYNCIFY_STATE: &str = "__asyncify_state";
pub const ASYNCIFY_DATA: &str = "__asyncify_data";
pub const ASYNCIFY_GET_STATE: &str = "asyncify_get_state";
pub const ASYNCIFY_START_UNWIND: &str = "asyncify_start_unwind";
pub const ASYNCIFY_STOP_UNWIND: &str = "asyncify_stop_unwind";
pub const ASYNCIFY_START_REWIND: &str = "asyncify_start_rewind";
pub const ASYNCIFY_STOP_REWIND: &str = "asyncify_stop_rewind";
pub const ASYNCIFY_UNWIND: &str = "asyncify_unwind";
pub const ASYNCIFY_GET_CALL_INDEX: &str = "asyncify_get_call_index";
pub const ASYNCIFY_CHECK_CALL_INDEX: &str = "asyncify_check_call_index";
pub const ASYNCIFY_GET_REWIND_INDEX: &str = "asyncify_get_rewind_index";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum AsyncifyState {
    Normal = 0,
    Unwinding = 1,
    Rewinding = 2,
}

#[derive(Debug, Default, Clone)]
pub struct AsyncifyConfig {
    pub imports: Vec<String>,
    pub ignore_imports: bool,
    pub ignore_indirect: bool,
    pub remove_list: Vec<String>,
    pub add_list: Vec<String>,
    pub only_list: Vec<String>,
    pub is_top_most_runtime: bool,
    pub is_bottom_most_runtime: bool,
}

pub struct Asyncify {
    config: AsyncifyConfig,
}

impl Asyncify {
    pub fn new(config: AsyncifyConfig) -> Self {
        Self { config }
    }

    fn add_globals(&self, module: &mut Module, pointer_type: Type) {
        if module.globals.iter().any(|(_, g)| g.name == ASYNCIFY_STATE) {
            return;
        }

        let (arena, _, _, globals, _, _, _, _, _, _, _, _, annotations) = module.parts_mut();
        let mut builder = IrBuilder::with_annotations(arena, annotations);

        globals.alloc(Global {
            name: ASYNCIFY_STATE.to_string(),
            type_: Type::I32,
            mutable: true,
            kind: GlobalKind::Local {
                init: builder.const_(Literal::I32(0)),
            },
        });

        if !globals.iter().any(|(_, g)| g.name == ASYNCIFY_DATA) {
            globals.alloc(Global {
                name: ASYNCIFY_DATA.to_string(),
                type_: pointer_type,
                mutable: true,
                kind: GlobalKind::Local {
                    init: builder.const_(if pointer_type == Type::I64 {
                        Literal::I64(0)
                    } else {
                        Literal::I32(0)
                    }),
                },
            });
        }
    }

    fn add_functions(&self, module: &mut Module, pointer_type: Type) {
        let (arena, _, functions, globals, _, _, _, _, _, _, _, _, annotations) =
            module.parts_mut();
        let mut builder = IrBuilder::with_annotations(arena, annotations);

        for &name in &[
            ASYNCIFY_UNWIND,
            ASYNCIFY_CHECK_CALL_INDEX,
            ASYNCIFY_GET_CALL_INDEX,
            ASYNCIFY_GET_REWIND_INDEX,
        ] {
            if functions.iter().any(|(_, f)| f.name == name) {
                continue;
            }

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
                builder.unreachable(),
            );
            functions.alloc(func);
        }

        let state_id = globals
            .iter()
            .find(|(_, g)| g.name == ASYNCIFY_STATE)
            .map(|(id, _)| id.cast())
            .unwrap();
        let data_id = globals
            .iter()
            .find(|(_, g)| g.name == ASYNCIFY_DATA)
            .map(|(id, _)| id.cast())
            .unwrap();

        if !functions
            .iter()
            .any(|(_, f)| f.name == ASYNCIFY_START_UNWIND)
        {
            self.make_helper(
                functions,
                arena,
                ASYNCIFY_START_UNWIND,
                true,
                AsyncifyState::Unwinding,
                pointer_type,
                state_id,
                data_id,
            );
            self.make_helper(
                functions,
                arena,
                ASYNCIFY_STOP_UNWIND,
                false,
                AsyncifyState::Normal,
                pointer_type,
                state_id,
                data_id,
            );
            self.make_helper(
                functions,
                arena,
                ASYNCIFY_START_REWIND,
                true,
                AsyncifyState::Rewinding,
                pointer_type,
                state_id,
                data_id,
            );
            self.make_helper(
                functions,
                arena,
                ASYNCIFY_STOP_REWIND,
                false,
                AsyncifyState::Normal,
                pointer_type,
                state_id,
                data_id,
            );
            self.make_helper(
                functions,
                arena,
                ASYNCIFY_GET_STATE,
                false,
                AsyncifyState::Normal,
                pointer_type,
                state_id,
                data_id,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn make_helper(
        &self,
        functions: &mut IdArena<crate::ir::module::FuncMarker, Function>,
        arena: &mut IdArena<crate::ir::expression::ExpressionMarker, Expression>,
        name: &str,
        set_data: bool,
        state: AsyncifyState,
        pointer_type: Type,
        state_id: crate::ir::GlobalId,
        data_id: crate::ir::GlobalId,
    ) {
        let mut builder = IrBuilder::new(arena);
        let mut list = Vec::new();

        if name == ASYNCIFY_GET_STATE {
            list.push(builder.global_get(state_id, Type::I32));
        } else {
            let state_const = builder.const_(Literal::I32(state as i32));
            list.push(builder.global_set(state_id, state_const));

            if set_data {
                let lget = builder.local_get(LocalId::from_u32(0), pointer_type);
                list.push(builder.global_set(data_id, lget));
            }
        }

        let body = builder.block(
            "",
            list,
            if name == ASYNCIFY_GET_STATE {
                Type::I32
            } else {
                Type::NONE
            },
        );
        let params = if set_data { pointer_type } else { Type::NONE };
        let results = if name == ASYNCIFY_GET_STATE {
            Type::I32
        } else {
            Type::NONE
        };
        let func = Function::new(name.to_string(), params, results, vec![], body);

        functions.alloc(func);
    }
}

pub fn dump_ir(
    arena: &IdArena<ExpressionMarker, Expression>,
    expr_id: ExpressionId,
    indent: usize,
) {
    let _indent_str = "  ".repeat(indent);
    // debug: dump removed
    match &arena[expr_id].kind {
        ExpressionKind::Block { list, .. } => {
            for child in list {
                dump_ir(arena, *child, indent + 1);
            }
        }
        ExpressionKind::Loop { body, .. } => {
            dump_ir(arena, *body, indent + 1);
        }
        ExpressionKind::If {
            condition,
            if_true,
            if_false,
            ..
        } => {
            dump_ir(arena, *condition, indent + 1);
            dump_ir(arena, *if_true, indent + 1);
            if let Some(f) = if_false {
                dump_ir(arena, *f, indent + 1);
            }
        }
        _ => {
            // For other nodes, we might want to see children too, but let's start with structural ones
            arena[expr_id]
                .kind
                .for_each_child(|child| dump_ir(arena, child, indent + 1));
        }
    }
}

impl Pass for Asyncify {
    fn name(&self) -> &str {
        "Asyncify"
    }

    fn run(&mut self, module: &mut Module) {
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

        // Export helper functions
        for name in &[
            ASYNCIFY_GET_STATE,
            ASYNCIFY_START_UNWIND,
            ASYNCIFY_STOP_UNWIND,
            ASYNCIFY_START_REWIND,
            ASYNCIFY_STOP_REWIND,
        ] {
            if let Some((id, _)) = module.get_function_by_name(name) {
                if !module.exports.iter().any(|e| e.name == *name) {
                    module.export_function(id, name.to_string());
                }
            }
        }

        let state_id = module
            .get_global_by_name(ASYNCIFY_STATE)
            .map(|(id, _)| id)
            .expect("Asyncify state global not found");

        let analyzer = ModuleAnalyzer::analyze(module, &self.config);

        let asyncify_funcs = AsyncifyFunctions {
            check_call_index: module
                .get_function_by_name(ASYNCIFY_CHECK_CALL_INDEX)
                .unwrap()
                .0,
            get_state: module.get_function_by_name(ASYNCIFY_GET_STATE).unwrap().0,
            get_rewind_index: module
                .get_function_by_name(ASYNCIFY_GET_REWIND_INDEX)
                .unwrap()
                .0,
            start_unwind: module
                .get_function_by_name(ASYNCIFY_START_UNWIND)
                .unwrap()
                .0,
            stop_unwind: module.get_function_by_name(ASYNCIFY_STOP_UNWIND).unwrap().0,
            start_rewind: module
                .get_function_by_name(ASYNCIFY_START_REWIND)
                .unwrap()
                .0,
            stop_rewind: module.get_function_by_name(ASYNCIFY_STOP_REWIND).unwrap().0,
            unwind: module.get_function_by_name(ASYNCIFY_UNWIND).unwrap().0,
        };

        let data_id = module
            .get_global_by_name(ASYNCIFY_DATA)
            .map(|(id, _)| id)
            .expect("Asyncify data global not found");

        let (arena, _, functions, _, _, _, _, _, _, _, _, _, annotations) = module.parts_mut();
        let ids: Vec<_> = functions.iter().map(|(id, _)| id).collect();
        for id in ids {
            if !analyzer.needs_instrumentation(id) {
                continue;
            }
            // instrumentation log removed

            if functions[id].name == "func_10" {
                let body_id = functions[id].body().unwrap();
                dump_ir(arena, body_id, 0);
            }

            let range = {
                let func = &mut functions[id];
                if let Some(mut body_id) = func.body() {
                    let mut flow = AsyncifyFlow::new(
                        arena,
                        &analyzer,
                        &asyncify_funcs,
                        func,
                        state_id,
                        annotations,
                    );
                    let r = flow.process(&mut body_id);
                    Some((r.0, r.1, body_id))
                } else {
                    None
                }
            };

            if let Some((start_idx, end_idx, body_id)) = range {
                functions[id].set_body(body_id);
                if start_idx != 0 || end_idx != 0 {
                    let mut locals = AsyncifyLocals::new(
                        arena,
                        functions,
                        &analyzer,
                        id,
                        state_id,
                        memory_id,
                        pointer_type,
                        data_id,
                        annotations,
                    );
                    locals.run();
                }
            }
        }
    }
}

// TODO: do NOT manipulate synthetic names!!

pub struct AsyncifyFunctions {
    pub check_call_index: FuncId,
    pub get_state: FuncId,
    pub get_rewind_index: FuncId,
    pub start_unwind: FuncId,
    pub stop_unwind: FuncId,
    pub start_rewind: FuncId,
    pub stop_rewind: FuncId,
    pub unwind: FuncId,
}

pub struct ModuleAnalyzer {
    pub can_change_state: HashSet<FuncId>,
    pub is_top_most_runtime: HashSet<FuncId>,
    pub is_bottom_most_runtime: HashSet<FuncId>,
    pub ignore_indirect: bool,
}

impl ModuleAnalyzer {
    pub fn analyze(module: &Module, config: &AsyncifyConfig) -> Self {
        let mut can_change_state = HashSet::new();
        let mut is_top_most_runtime = HashSet::new();
        let mut is_bottom_most_runtime = HashSet::new();

        for (id, func) in module.functions.iter() {
            // Asyncify analysis log removed
            let mut analyzer = FunctionStateAnalyzer {
                module,
                config,
                can_change_state: false,
                is_top_most_runtime: false,
                is_bottom_most_runtime: false,
            };
            if let Some(body) = func.body() {
                analyzer.visit(body, &module.expressions);
            }

            if analyzer.can_change_state {
                can_change_state.insert(id);
            }

            if let FunctionKind::Import(import) = &func.kind {
                let mod_name = &import.module;
                let func_name = &import.name;
                let is_asyncify = mod_name == "asyncify";
                let mut changes = false;
                if is_asyncify {
                    match func_name.as_str() {
                        "start_unwind" | "stop_rewind" => {
                            changes = true;
                            is_top_most_runtime.insert(id);
                        }
                        "stop_unwind" | "start_rewind" => {
                            is_bottom_most_runtime.insert(id);
                        }
                        _ => {}
                    }
                } else if !config.ignore_imports
                    && (config.imports.is_empty()
                        || config
                            .imports
                            .contains(&format!("{}.{}", mod_name, func_name)))
                {
                    changes = true;
                }
                if changes {
                    // import marking log removed
                    can_change_state.insert(id);
                }
            }
        }

        let call_graph = CallGraph::build(module);
        let final_can_change_state = call_graph.propagate_back(can_change_state, |id| {
            let func = &module.functions[id];
            !config.remove_list.contains(&func.name.to_string())
                && !is_bottom_most_runtime.contains(&id)
        });

        Self {
            can_change_state: final_can_change_state,
            is_top_most_runtime,
            is_bottom_most_runtime,
            ignore_indirect: config.ignore_indirect,
        }
    }

    pub fn needs_instrumentation(&self, id: FuncId) -> bool {
        self.can_change_state.contains(&id) && !self.is_top_most_runtime.contains(&id)
    }

    pub fn can_change_state_expr(&self, module: &Module, expr: ExpressionId) -> bool {
        self.can_change_state_expr_inner(&module.expressions, expr)
    }

    pub fn can_change_state_expr_inner(
        &self,
        arena: &IdArena<ExpressionMarker, Expression>,
        expr: ExpressionId,
    ) -> bool {
        let mut visitor = CanChangeStateVisitor {
            analyzer: self,
            can_change_state: false,
        };
        visitor.visit(expr, arena);
        visitor.can_change_state
    }
}

struct CanChangeStateVisitor<'a> {
    analyzer: &'a ModuleAnalyzer,
    can_change_state: bool,
}

impl<'a> ReadOnlyVisitor for CanChangeStateVisitor<'a> {
    fn visit_expression(
        &mut self,
        expr: ExpressionId,
        arena: &IdArena<ExpressionMarker, Expression>,
    ) {
        if self.can_change_state {
            return;
        }
        let expression = &arena[expr];
        match &expression.kind {
            ExpressionKind::Call { target, .. } => {
                // visiting call
                if self.analyzer.can_change_state.contains(target) {
                    // call can change state
                    self.can_change_state = true;
                }
            }
            ExpressionKind::CallIndirect { .. } if !self.analyzer.ignore_indirect => {
                // indirect call can change state
                self.can_change_state = true
            }
            _ => {
                self.visit_children(expr, arena);
            }
        }
    }
}

struct FunctionStateAnalyzer<'a> {
    module: &'a Module,
    config: &'a AsyncifyConfig,
    can_change_state: bool,
    is_top_most_runtime: bool,
    is_bottom_most_runtime: bool,
}

impl<'a> ReadOnlyVisitor for FunctionStateAnalyzer<'a> {
    fn visit_expression(
        &mut self,
        expr: ExpressionId,
        arena: &IdArena<ExpressionMarker, Expression>,
    ) {
        let expression = &arena[expr];
        match &expression.kind {
            ExpressionKind::Call { target, .. } => {
                let func = &self.module.functions[*target];
                if let FunctionKind::Import(import) = &func.kind {
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
            ExpressionKind::CallIndirect { .. } if !self.config.ignore_indirect => {
                self.can_change_state = true;
            }
            _ => {}
        }
        self.visit_children(expr, arena);
    }
}

pub struct AsyncifyFlow<'a> {
    arena: &'a mut IdArena<ExpressionMarker, Expression>,
    analyzer: &'a ModuleAnalyzer,
    state_id: crate::ir::GlobalId,
    funcs: &'a AsyncifyFunctions,
    func: &'a mut Function,
    call_index: u32,
    pub site_liveness: HashMap<u32, HashSet<LocalId>>,
    annotations: &'a mut crate::ir::annotation::AnnotationStore,
}

impl<'a> AsyncifyFlow<'a> {
    pub fn new(
        arena: &'a mut IdArena<ExpressionMarker, Expression>,
        analyzer: &'a ModuleAnalyzer,
        funcs: &'a AsyncifyFunctions,
        func: &'a mut Function,
        state_id: crate::ir::GlobalId,
        annotations: &'a mut crate::ir::annotation::AnnotationStore,
    ) -> Self {
        Self {
            arena,
            analyzer,
            state_id,
            funcs,
            func,
            call_index: 0,
            site_liveness: HashMap::new(),
            annotations,
        }
    }

    fn add_local(&mut self, ty: Type) -> LocalId {
        self.func.add_var(ty)
    }

    fn make_state_check(&mut self, state: AsyncifyState, ref_id: ExpressionId) -> ExpressionId {
        let sid = self.state_id;
        let mut builder = IrBuilder::with_annotations(self.arena, self.annotations);
        builder.copy_location(ref_id);
        let gget = builder.global_get(sid, Type::I32);
        let cst = builder.const_(Literal::I32(state as i32));
        builder.binary(crate::ir::ops::BinaryOp::EqInt32, gget, cst, Type::I32)
    }

    fn make_range_check(&mut self, start: u32, end: u32, ref_id: ExpressionId) -> ExpressionId {
        let rewind_id = self.funcs.get_rewind_index;
        let mut builder = IrBuilder::with_annotations(self.arena, self.annotations);
        builder.copy_location(ref_id);
        let get_rewind_index = builder.call(rewind_id, vec![], Type::I32, false);

        let cstart = builder.const_(Literal::I32(start as i32));
        let ge = builder.binary(
            crate::ir::ops::BinaryOp::GeUInt32,
            get_rewind_index,
            cstart,
            Type::I32,
        );
        let cend = builder.const_(Literal::I32(end as i32));
        let lt = builder.binary(
            crate::ir::ops::BinaryOp::LeUInt32,
            get_rewind_index,
            cend,
            Type::I32,
        );

        builder.binary(crate::ir::ops::BinaryOp::AndInt32, ge, lt, Type::I32)
    }

    pub fn process(&mut self, expr_id: &mut ExpressionId) -> (u32, u32) {
        if !self
            .analyzer
            .can_change_state_expr_inner(self.arena, *expr_id)
        {
            let old = *expr_id;
            let start = self.call_index;
            let cond = self.make_state_check(AsyncifyState::Normal, old);
            let ty = self.arena[old].type_;
            let mut builder = IrBuilder::with_annotations(self.arena, self.annotations);
            builder.copy_location(old);
            *expr_id = builder.if_("", cond, old, None, ty);
            return (start, self.call_index);
        }
        let start_index = self.call_index;
        let kind = self.arena[*expr_id].kind.clone();
        match kind {
            ExpressionKind::Block { mut list, .. } => {
                for child in list.iter_mut() {
                    self.process(child);
                }
                if let ExpressionKind::Block { list: old_list, .. } = &mut self.arena[*expr_id].kind
                {
                    *old_list = list;
                }
            }
            ExpressionKind::If {
                name: _,
                condition,
                mut if_true,
                mut if_false,
            } => {
                let old_id = *expr_id;
                let true_range = self.process(&mut if_true);
                let _false_range = if_false.as_mut().map(|f| self.process(f));

                let state_rewinding = self.make_state_check(AsyncifyState::Rewinding, old_id);

                let in_true_range = if true_range.1 > 0 {
                    self.make_range_check(true_range.0, true_range.1, old_id)
                } else {
                    let mut b = IrBuilder::with_annotations(self.arena, self.annotations);
                    b.copy_location(old_id);
                    b.const_(Literal::I32(0))
                };

                let mut b = IrBuilder::with_annotations(self.arena, self.annotations);
                b.copy_location(old_id);
                let new_cond = b.if_(
                    "",
                    state_rewinding,
                    in_true_range,
                    Some(condition),
                    Type::I32,
                );

                if let ExpressionKind::If {
                    name: _,
                    condition: old_cond,
                    if_true: old_true,
                    if_false: old_false,
                    ..
                } = &mut self.arena[*expr_id].kind
                {
                    *old_cond = new_cond;
                    *old_true = if_true;
                    *old_false = if_false;
                }
            }
            ExpressionKind::Loop { mut body, name } => {
                let name = name.clone();
                let ty = self.arena[body].type_;
                let range = self.process(&mut body);
                let state_normal = self.make_state_check(AsyncifyState::Normal, *expr_id);
                let state_rewinding = self.make_state_check(AsyncifyState::Rewinding, *expr_id);
                let in_range = if range.1 > range.0 {
                    self.make_range_check(range.0, range.1 - 1, *expr_id)
                } else {
                    let mut b = IrBuilder::with_annotations(self.arena, self.annotations);
                    b.copy_location(*expr_id);
                    b.const_(Literal::I32(0))
                };

                let mut b = IrBuilder::with_annotations(self.arena, self.annotations);
                b.copy_location(*expr_id);
                let and_rewind = b.binary(
                    crate::ir::ops::BinaryOp::AndInt32,
                    state_rewinding,
                    in_range,
                    Type::I32,
                );
                let or_normal = b.binary(
                    crate::ir::ops::BinaryOp::OrInt32,
                    state_normal,
                    and_rewind,
                    Type::I32,
                );

                let if_body_id = b.if_("", or_normal, body, None, ty);
                // Directly set the expression kind and body
                self.arena[*expr_id].kind = ExpressionKind::Loop {
                    body: if_body_id,
                    name,
                };
            }
            ExpressionKind::Call {
                target, is_return, ..
            } => {
                if self.analyzer.can_change_state.contains(&target) {
                    if is_return {
                        // For return_call, we need to stop rewinding BEFORE the tail call
                        *expr_id = self.make_tail_call_support(*expr_id);
                    } else {
                        *expr_id = self.make_call_support(*expr_id);
                    }
                }
            }
            ExpressionKind::CallIndirect { .. } => {
                if !self.analyzer.ignore_indirect {
                    *expr_id = self.make_call_support(*expr_id)
                }
            }
            ExpressionKind::LocalSet { value, .. }
            | ExpressionKind::LocalTee { value, .. }
            | ExpressionKind::Drop { value } => {
                let mut v = value;
                if self.analyzer.can_change_state_expr_inner(self.arena, v) {
                    self.process(&mut v);
                    match &mut self.arena[*expr_id].kind {
                        ExpressionKind::LocalSet { value: old_v, .. }
                        | ExpressionKind::LocalTee { value: old_v, .. }
                        | ExpressionKind::Drop { value: old_v } => {
                            *old_v = v;
                        }
                        _ => unreachable!(),
                    }
                } else {
                    let old = *expr_id;
                    let cond = self.make_state_check(AsyncifyState::Normal, old);
                    let ty = self.arena[old].type_;
                    let mut b = IrBuilder::with_annotations(self.arena, self.annotations);
                    b.copy_location(old);
                    *expr_id = b.if_("", cond, old, None, ty);
                }
            }
            _ => {
                let old = *expr_id;
                let cond = self.make_state_check(AsyncifyState::Normal, old);
                let ty = self.arena[old].type_;
                let mut b = IrBuilder::with_annotations(self.arena, self.annotations);
                b.copy_location(old);
                *expr_id = b.if_("", cond, old, None, ty);
            }
        }
        (start_index, self.call_index)
    }

    fn make_call_support(&mut self, expr_id: ExpressionId) -> ExpressionId {
        let index = self.call_index;
        self.call_index += 1;

        let site_label = format!("asyncify_site_{}", index);

        let check_call_id = self.funcs.check_call_index;
        let unwind_id = self.funcs.unwind;

        let check_index = {
            let mut builder = IrBuilder::with_annotations(self.arena, self.annotations);
            builder.copy_location(expr_id);
            let cidx = builder.const_(Literal::I32(index as i32));
            builder.call(check_call_id, vec![cidx], Type::I32, false)
        };

        let state_normal = self.make_state_check(AsyncifyState::Normal, expr_id);

        let condition = {
            let mut builder = IrBuilder::with_annotations(self.arena, self.annotations);
            builder.copy_location(expr_id);
            builder.binary(
                crate::ir::ops::BinaryOp::OrInt32,
                state_normal,
                check_index,
                Type::I32,
            )
        };

        let state_unwinding = self.make_state_check(AsyncifyState::Unwinding, expr_id);

        let ucall = {
            let mut builder = IrBuilder::with_annotations(self.arena, self.annotations);
            builder.copy_location(expr_id);
            let cidx2 = builder.const_(Literal::I32(index as i32));
            builder.call(unwind_id, vec![cidx2], Type::NONE, false)
        };
        let unwind_check = {
            let mut builder = IrBuilder::with_annotations(self.arena, self.annotations);
            builder.copy_location(expr_id);
            builder.if_("", state_unwinding, ucall, None, Type::NONE)
        };

        let state_rewinding = self.make_state_check(AsyncifyState::Rewinding, expr_id);

        let result_type = self.arena[expr_id].type_;
        let temp_local = if result_type != Type::NONE && result_type != Type::UNREACHABLE {
            Some(self.add_local(result_type))
        } else {
            None
        };

        let stop_rewind_if_needed = {
            let mut builder = IrBuilder::with_annotations(self.arena, self.annotations);
            builder.copy_location(expr_id);
            let sid = self.state_id;
            let cnorm = builder.const_(Literal::I32(AsyncifyState::Normal as i32));
            let gset = builder.global_set(sid, cnorm);
            builder.if_("", state_rewinding, gset, None, Type::NONE)
        };

        let mut builder = IrBuilder::with_annotations(self.arena, self.annotations);
        builder.copy_location(expr_id);
        let mut list = Vec::new();
        if let Some(local) = temp_local {
            list.push(builder.local_set(local, expr_id));
            list.push(unwind_check);
            list.push(stop_rewind_if_needed);
            list.push(builder.local_get(local, result_type));
        } else {
            list.push(expr_id);
            list.push(unwind_check);
            list.push(stop_rewind_if_needed);
        }

        let block = builder.block(Some(site_label), list, result_type);
        builder.if_("", condition, block, None, result_type)
    }

    fn make_tail_call_support(&mut self, expr_id: ExpressionId) -> ExpressionId {
        let index = self.call_index;
        self.call_index += 1;

        let check_call_id = self.funcs.check_call_index;
        let unwind_id = self.funcs.unwind;

        let check_index = {
            let mut builder = IrBuilder::with_annotations(self.arena, self.annotations);
            builder.copy_location(expr_id);
            let cidx = builder.const_(Literal::I32(index as i32));
            builder.call(check_call_id, vec![cidx], Type::I32, false)
        };

        let state_normal = self.make_state_check(AsyncifyState::Normal, expr_id);

        let condition = {
            let mut builder = IrBuilder::with_annotations(self.arena, self.annotations);
            builder.copy_location(expr_id);
            builder.binary(
                crate::ir::ops::BinaryOp::OrInt32,
                state_normal,
                check_index,
                Type::I32,
            )
        };

        let state_unwinding = self.make_state_check(AsyncifyState::Unwinding, expr_id);

        let ucall = {
            let mut builder = IrBuilder::with_annotations(self.arena, self.annotations);
            builder.copy_location(expr_id);
            let cidx2 = builder.const_(Literal::I32(index as i32));
            builder.call(unwind_id, vec![cidx2], Type::NONE, false)
        };
        let unwind_check = {
            let mut builder = IrBuilder::with_annotations(self.arena, self.annotations);
            builder.copy_location(expr_id);
            builder.if_("", state_unwinding, ucall, None, Type::NONE)
        };

        let state_rewinding = self.make_state_check(AsyncifyState::Rewinding, expr_id);

        let stop_rewind_if_needed = {
            let mut builder = IrBuilder::with_annotations(self.arena, self.annotations);
            builder.copy_location(expr_id);
            let sid = self.state_id;
            let cnorm = builder.const_(Literal::I32(AsyncifyState::Normal as i32));
            let gset = builder.global_set(sid, cnorm);
            builder.if_("", state_rewinding, gset, None, Type::NONE)
        };

        let mut builder = IrBuilder::with_annotations(self.arena, self.annotations);
        builder.copy_location(expr_id);
        // For tail call, we MUST stop rewinding before the call,
        // because the tail call replaces the current frame.
        let list = vec![stop_rewind_if_needed, expr_id, unwind_check];

        let block = builder.block(Option::<String>::None, list, Type::UNREACHABLE);
        builder.if_("", condition, block, None, Type::UNREACHABLE)
    }
}

pub struct AsyncifyLocals<'a> {
    arena: &'a mut IdArena<ExpressionMarker, Expression>,
    functions: &'a mut IdArena<FuncMarker, Function>,
    analyzer: &'a ModuleAnalyzer,
    func_id: FuncId,
    state_id: crate::ir::GlobalId,
    memory_id: crate::ir::MemoryId,
    pointer_type: Type,
    data_id: crate::ir::GlobalId,
    annotations: &'a mut crate::ir::annotation::AnnotationStore,
}

impl<'a> AsyncifyLocals<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        arena: &'a mut IdArena<ExpressionMarker, Expression>,
        functions: &'a mut IdArena<FuncMarker, Function>,
        analyzer: &'a ModuleAnalyzer,
        func_id: FuncId,
        state_id: crate::ir::GlobalId,
        memory_id: crate::ir::MemoryId,
        pointer_type: Type,
        data_id: crate::ir::GlobalId,
        annotations: &'a mut crate::ir::annotation::AnnotationStore,
    ) -> Self {
        Self {
            arena,
            functions,
            analyzer,
            func_id,
            state_id,
            memory_id,
            pointer_type,
            data_id,
            annotations,
        }
    }

    pub fn run(&mut self) {
        let asyncify_data_global_id = self.data_id;
        let memory_id = self.memory_id;
        let state_id = self.state_id;
        let pointer_type = self.pointer_type;

        let arena = &mut *self.arena;
        let functions = &mut *self.functions;

        {
            let func = &mut functions[self.func_id];
            if let Some(mut body_id) = func.body() {
                ReFinalize::run(&mut body_id, arena);
            } else {
                return;
            }
        }

        let liveness = Liveness::new(&functions[self.func_id], arena);
        let mut relevant_live_locals = HashSet::new();

        struct CallLivenessScanner<'a> {
            relevant_live_locals: &'a mut HashSet<LocalId>,
            live_now: HashSet<LocalId>,
            analyzer: &'a ModuleAnalyzer,
        }

        impl<'a> ReadOnlyVisitor for CallLivenessScanner<'a> {
            fn visit_expression(
                &mut self,
                expr: ExpressionId,
                arena: &IdArena<ExpressionMarker, Expression>,
            ) {
                match &arena[expr].kind {
                    ExpressionKind::LocalSet { id, value }
                    | ExpressionKind::LocalTee { id, value } => {
                        self.visit(*value, arena);
                        self.live_now.insert(*id);
                    }
                    ExpressionKind::LocalGet { id } => {
                        self.live_now.insert(*id);
                    }
                    ExpressionKind::Call { target, .. } => {
                        if self.analyzer.can_change_state.contains(target) {
                            self.relevant_live_locals.extend(&self.live_now);
                        }
                    }
                    ExpressionKind::CallIndirect { .. } => {
                        if !self.analyzer.ignore_indirect {
                            self.relevant_live_locals.extend(&self.live_now);
                        }
                    }
                    _ => {}
                }
            }
        }

        for (block_id, block) in liveness.cfg.blocks.iter() {
            let mut scanner = CallLivenessScanner {
                relevant_live_locals: &mut relevant_live_locals,
                live_now: liveness.live_in[&block_id].clone(),
                analyzer: self.analyzer,
            };
            for &expr_id in &block.contents {
                scanner.visit(expr_id, arena);
            }
        }

        let mut live_locals_vec: Vec<LocalId> = relevant_live_locals.into_iter().collect();
        live_locals_vec.sort();

        let rewind_index_local: LocalId;
        let unwind_index_local: LocalId;
        let new_inner_body;

        {
            let func = &mut functions[self.func_id];
            rewind_index_local = func.add_var(Type::I32);
            unwind_index_local = func.add_var(Type::I32);
        }

        let unwind_block_id = arena.alloc(Expression {
            kind: ExpressionKind::Nop,
            type_: Type::NONE,
        });

        {
            let mut transformer = LocalTransformer {
                arena,
                rewind_index_local,
                unwind_index_local,
                unwind_target: unwind_block_id,
                functions,
                annotations: self.annotations,
            };
            let mut body = functions[self.func_id].body().unwrap();
            transformer.process(&mut body);
            new_inner_body = body;
        }

        let func = &mut functions[self.func_id];
        let mut builder = IrBuilder::with_annotations(arena, self.annotations);
        if let Some(body_id) = func.body() {
            builder.copy_location(body_id);
        }
        let data_ptr = builder.global_get(asyncify_data_global_id, pointer_type);

        let mut load_block_list = Vec::new();
        let mut total_locals_size = 0;
        if !live_locals_vec.is_empty() {
            for &local_idx in &live_locals_vec {
                let ty = func.get_local_type(local_idx);
                total_locals_size += match ty {
                    Type::I64 | Type::F64 => 8,
                    _ => 4,
                };
            }

            let current_ptr = builder.load(memory_id, 4, false, 0, 4, data_ptr, Type::I32);
            let size_const = builder.const_(Literal::I32(total_locals_size));
            let new_ptr = builder.binary(
                crate::ir::ops::BinaryOp::SubInt32,
                current_ptr,
                size_const,
                Type::I32,
            );
            load_block_list.push(builder.store(memory_id, 4, 0, 4, data_ptr, new_ptr));

            let stack_ptr_temp = func.add_var(pointer_type);
            let loaded_val = builder.load(memory_id, 4, false, 0, 4, data_ptr, Type::I32);
            load_block_list.push(builder.local_set(stack_ptr_temp, loaded_val));

            let mut offset = 0;
            for &local_idx in &live_locals_vec {
                let ty = func.get_local_type(local_idx);
                let size = match ty {
                    Type::I64 | Type::F64 => 8,
                    _ => 4,
                };

                let ptr_get = builder.local_get(stack_ptr_temp, pointer_type);
                let loaded_val = builder.load(memory_id, size, true, offset, 4, ptr_get, ty);
                load_block_list.push(builder.local_set(local_idx, loaded_val));
                offset += size;
            }
        }
        let load_block = builder.block("", load_block_list, Type::NONE);

        let mut save_block_list = Vec::new();
        if !live_locals_vec.is_empty() {
            let stack_ptr_temp = func.add_var(pointer_type);
            let loaded_val = builder.load(memory_id, 4, false, 0, 4, data_ptr, Type::I32);
            save_block_list.push(builder.local_set(stack_ptr_temp, loaded_val));

            let mut offset = 0;
            for &local_idx in &live_locals_vec {
                let ty = func.get_local_type(local_idx);
                let size = match ty {
                    Type::I64 | Type::F64 => 8,
                    _ => 4,
                };

                let ptr_get = builder.local_get(stack_ptr_temp, pointer_type);
                let val_get = builder.local_get(local_idx, ty);
                save_block_list.push(builder.store(memory_id, size, offset, 4, ptr_get, val_get));
                offset += size;
            }

            let ptr_get = builder.local_get(stack_ptr_temp, pointer_type);
            let size_const = builder.const_(Literal::I32(total_locals_size));
            let new_ptr = builder.binary(
                crate::ir::ops::BinaryOp::AddInt32,
                ptr_get,
                size_const,
                Type::I32,
            );
            save_block_list.push(builder.store(memory_id, 4, 0, 4, data_ptr, new_ptr));
        }
        let save_block = builder.block("", save_block_list, Type::NONE);

        let mut list = Vec::new();
        let gget_state = builder.global_get(state_id, Type::I32);
        let crewind = builder.const_(Literal::I32(AsyncifyState::Rewinding as i32));
        let rewinding = builder.binary(
            crate::ir::ops::BinaryOp::EqInt32,
            gget_state,
            crewind,
            Type::I32,
        );
        let mut rewind_then = Vec::new();

        rewind_then.push(load_block);

        let load_ptr = builder.load(memory_id, 4, false, 0, 4, data_ptr, Type::I32);
        let c4 = builder.const_(Literal::I32(4));
        let sub = builder.binary(crate::ir::ops::BinaryOp::SubInt32, load_ptr, c4, Type::I32);
        rewind_then.push(builder.store(memory_id, 4, 0, 4, data_ptr, sub));
        let load_idx = builder.load(memory_id, 4, false, 0, 4, data_ptr, Type::I32);
        rewind_then.push(builder.local_set(rewind_index_local, load_idx));

        let r_block = builder.block("", rewind_then, Type::NONE);
        list.push(builder.if_("", rewinding, r_block, None, Type::NONE));

        let unwind_block_list = vec![new_inner_body];

        builder.arena[unwind_block_id].kind = ExpressionKind::Block {
            name: Some(ASYNCIFY_UNWIND.to_string()),
            list: unwind_block_list,
        };
        builder.arena[unwind_block_id].type_ = Type::NONE;
        list.push(unwind_block_id);

        let gget_state2 = builder.global_get(state_id, Type::I32);
        let cunwind = builder.const_(Literal::I32(AsyncifyState::Unwinding as i32));
        let unwinding = builder.binary(
            crate::ir::ops::BinaryOp::EqInt32,
            gget_state2,
            cunwind,
            Type::I32,
        );
        let mut unwind_then = Vec::new();

        let total_frame_size = 4 + total_locals_size;
        let stack_ptr_temp = func.add_var(pointer_type);
        let loaded_ptr = builder.load(memory_id, 4, false, 0, 4, data_ptr, Type::I32);
        unwind_then.push(builder.local_set(stack_ptr_temp, loaded_ptr));
        let limit = builder.load(memory_id, 4, false, 4, 4, data_ptr, Type::I32);
        let ptr_get = builder.local_get(stack_ptr_temp, pointer_type);
        let frame_size_const = builder.const_(Literal::I32(total_frame_size));
        let next_ptr = builder.binary(
            crate::ir::ops::BinaryOp::AddInt32,
            ptr_get,
            frame_size_const,
            Type::I32,
        );
        let gt = builder.binary(
            crate::ir::ops::BinaryOp::GtUInt32,
            next_ptr,
            limit,
            Type::I32,
        );
        let unreach = builder.unreachable();
        unwind_then.push(builder.if_("", gt, unreach, None, Type::NONE));

        let ptr_get2 = builder.local_get(stack_ptr_temp, pointer_type);
        let idx_get = builder.local_get(unwind_index_local, Type::I32);
        unwind_then.push(builder.store(memory_id, 4, 0, 4, ptr_get2, idx_get));
        let ptr_get3 = builder.local_get(stack_ptr_temp, pointer_type);
        let c4 = builder.const_(Literal::I32(4));
        let next_ptr2 = builder.binary(crate::ir::ops::BinaryOp::AddInt32, ptr_get3, c4, Type::I32);
        unwind_then.push(builder.store(memory_id, 4, 0, 4, data_ptr, next_ptr2));

        unwind_then.push(save_block);

        let u_block = builder.block("", unwind_then, Type::NONE);
        list.push(builder.if_("", unwinding, u_block, None, Type::NONE));

        func.set_body(builder.block("", list, Type::NONE));
    }
}

struct LocalTransformer<'a> {
    arena: &'a mut IdArena<ExpressionMarker, Expression>,
    rewind_index_local: LocalId,
    unwind_index_local: LocalId,
    unwind_target: ExpressionId,
    functions: &'a IdArena<FuncMarker, Function>,
    annotations: &'a mut AnnotationStore,
}

impl<'a> LocalTransformer<'a> {
    fn process(&mut self, expr_id: &mut ExpressionId) {
        let kind = self.arena[*expr_id].kind.clone();
        match kind {
            ExpressionKind::Call {
                target,
                mut operands,
                is_return: _,
            } => {
                let target_name = &self.functions[target].name;
                if target_name == ASYNCIFY_UNWIND {
                    let mut builder = IrBuilder::with_annotations(self.arena, self.annotations);
                    builder.copy_location(*expr_id);
                    let capture = builder.local_set(self.unwind_index_local, operands[0]);
                    let br = builder.break_(self.unwind_target, None, None, Type::UNREACHABLE);
                    *expr_id = builder.block("", vec![capture, br], Type::UNREACHABLE);
                } else if target_name == ASYNCIFY_CHECK_CALL_INDEX {
                    let mut builder = IrBuilder::with_annotations(self.arena, self.annotations);
                    builder.copy_location(*expr_id);
                    let lget = builder.local_get(self.rewind_index_local, Type::I32);
                    *expr_id = builder.binary(
                        crate::ir::ops::BinaryOp::EqInt32,
                        lget,
                        operands[0],
                        Type::I32,
                    );
                } else if target_name == ASYNCIFY_GET_REWIND_INDEX {
                    let mut builder = IrBuilder::with_annotations(self.arena, self.annotations);
                    builder.copy_location(*expr_id);
                    *expr_id = builder.local_get(self.rewind_index_local, Type::I32);
                } else if target_name == ASYNCIFY_GET_CALL_INDEX {
                    let mut builder = IrBuilder::with_annotations(self.arena, self.annotations);
                    builder.copy_location(*expr_id);
                    *expr_id = builder.const_(Literal::I32(0));
                } else {
                    for op in operands.iter_mut() {
                        self.process(op);
                    }
                    if let ExpressionKind::Call {
                        operands: old_operands,
                        ..
                    } = &mut self.arena[*expr_id].kind
                    {
                        *old_operands = operands;
                    }
                }
            }
            _ => {
                let mut kind = self.arena[*expr_id].kind.clone();
                kind.for_each_child_mut(|child| {
                    self.process(child);
                });
                self.arena[*expr_id].kind = kind;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::module::Module;

    #[test]
    fn test_asyncify_basic() {
        let mut module = Module::new();
        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        let mut builder = IrBuilder::new(arena);

        // Define imports that change state
        let sleep_id = functions.alloc(Function {
            name: "sleep".to_string(),
            params: Type::NONE,
            results: Type::NONE,
            type_idx: None,
            kind: crate::ir::module::FunctionKind::Import(crate::ir::module::FunctionImport {
                module: "env".to_string(),
                name: "sleep".to_string(),
            }),
        });

        let sleep_call = builder.call(sleep_id, vec![], Type::NONE, false);
        let body = builder.block("", vec![sleep_call], Type::NONE);
        let func = Function::new(
            "test_func".to_string(),
            Type::NONE,
            Type::NONE,
            Vec::new(),
            body,
        );

        functions.alloc(func);

        let mut config = AsyncifyConfig::default();
        config.imports.push("env.sleep".to_string());

        let mut asyncify = Asyncify::new(config);
        asyncify.run(&mut module);

        // Verification: Check if globals were added
        assert!(module.globals.iter().any(|(_, g)| g.name == ASYNCIFY_STATE));
        assert!(module.globals.iter().any(|(_, g)| g.name == ASYNCIFY_DATA));

        // The function body should have been wrapped in a block
        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        let func_id = functions
            .iter()
            .find(|(_, f)| f.name == "test_func")
            .map(|(id, _)| id)
            .unwrap();
        let func = &functions[func_id];
        let body_id = func.body().unwrap();
        if let ExpressionKind::Block { list, .. } = &arena[body_id].kind {
            assert!(list.len() >= 2);
        } else {
            panic!("Function body should be a block now");
        }
    }
}
