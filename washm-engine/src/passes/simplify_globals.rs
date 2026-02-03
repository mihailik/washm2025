use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::module::Module;
use crate::ir::visitor::Visitor;
use crate::ir::{GlobalId, Literal};
use crate::passes::Pass;
use std::collections::HashMap;

pub struct SimplifyGlobals;

impl Pass for SimplifyGlobals {
    fn name(&self) -> &str {
        "SimplifyGlobals"
    }

    fn run(&mut self, module: &mut Module) {
        // 1. Constantize globals: Find globals that are only written with constants
        self.constantize_globals(module);

        // 2. Remove unused globals: Find globals that are never read
        self.remove_unused_globals(module);

        // 3. Constant propagation for globals
        self.propagate_globals(module);
    }
}

impl SimplifyGlobals {
    fn constantize_globals(&mut self, module: &mut Module) {
        let mut global_info = HashMap::new();

        let (arena, _, functions, globals, _, _, _, _, _, _, _, _, _) = module.parts_mut();

        for (id, global) in globals.iter() {
            let gid = id.cast();
            global_info.insert(gid, GlobalUsageInfo::new());
            if let Some(init) = global.kind_to_init() {
                if let ExpressionKind::Const(lit) = &arena[init].kind {
                    global_info.get_mut(&gid).unwrap().initial_constant = Some(*lit);
                }
            }
        }

        for (_, func) in functions.iter() {
            if let Some(mut body) = func.body() {
                let mut scanner = GlobalUsageScanner {
                    infos: &mut global_info,
                };
                scanner.visit(&mut body, arena);
            }
        }

        let mut constants = HashMap::new();
        for (id, info) in global_info {
            if !info.is_written {
                if let Some(lit) = info.initial_constant {
                    constants.insert(id, lit);
                }
            }
        }

        if !constants.is_empty() {
            for (_, func) in functions.iter_mut() {
                if let Some(mut body) = func.body() {
                    let mut folder = GlobalFolder {
                        constants: &constants,
                    };
                    folder.visit(&mut body, arena);
                }
            }
        }
    }

    fn remove_unused_globals(&mut self, _module: &mut Module) {
        // Implementation for removing unused globals
    }

    fn propagate_globals(&mut self, _module: &mut Module) {
        // Implementation for global constant propagation
    }
}

#[derive(Debug)]
struct GlobalUsageInfo {
    is_read: bool,
    is_written: bool,
    initial_constant: Option<Literal>,
}

impl GlobalUsageInfo {
    fn new() -> Self {
        Self {
            is_read: false,
            is_written: false,
            initial_constant: None,
        }
    }
}

struct GlobalUsageScanner<'a> {
    infos: &'a mut HashMap<GlobalId, GlobalUsageInfo>,
}

impl<'a> Visitor for GlobalUsageScanner<'a> {
    fn visit(
        &mut self,
        expr_id: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        match &arena[*expr_id].kind {
            ExpressionKind::GlobalGet { id } => {
                let gid = *id;
                if let Some(info) = self.infos.get_mut(&gid) {
                    info.is_read = true;
                }
            }
            ExpressionKind::GlobalSet { id, .. } => {
                let gid = *id;
                if let Some(info) = self.infos.get_mut(&gid) {
                    info.is_written = true;
                }
            }
            _ => {}
        }
        self.visit_children(expr_id, arena);
    }
}

struct GlobalFolder<'a> {
    constants: &'a HashMap<GlobalId, Literal>,
}

impl<'a> Visitor for GlobalFolder<'a> {
    fn visit(
        &mut self,
        expr_id: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        if let ExpressionKind::GlobalGet { id } = &arena[*expr_id].kind {
            if let Some(lit) = self.constants.get(id) {
                arena[*expr_id].kind = ExpressionKind::Const(*lit);
            }
        }
        self.visit_children(expr_id, arena);
    }
}
