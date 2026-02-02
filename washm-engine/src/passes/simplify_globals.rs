use crate::ir::expression::{ExprRef, ExpressionKind, IrBuilder};
use crate::ir::module::{ExportKind, Module};
use crate::ir::visitor::Visitor;
use crate::ir::Literal;
use crate::passes::Pass;
use std::collections::{HashMap, HashSet};

pub struct SimplifyGlobals;

impl Default for SimplifyGlobals {
    fn default() -> Self {
        Self
    }
}

impl SimplifyGlobals {
    pub fn new() -> Self {
        Self
    }

    fn iteration<'a>(&mut self, module: &mut Module<'a>) -> bool {
        let mut tracker = GlobalUsageTracker::new(module);
        tracker.track();
        let infos = tracker.infos;

        let mut changed = false;

        // 0. Mark immutable in practice
        for (id, global) in module.globals.iter_mut() {
            let info = &infos[&id.cast()];
            if global.mutable && !info.exported && info.written == 0 {
                global.mutable = false;
                changed = true;
            }
        }

        // 1. Single-use folding
        changed |= self.fold_single_uses(module, &infos);

        // 2. Remove unneeded writes
        let (more, to_remove_sets) = self.remove_unneeded_writes(module, &infos);
        if !to_remove_sets.is_empty() {
            self.apply_set_removal(module, to_remove_sets);
            changed = true;
        }

        // 3. Constant propagation to globals
        changed |= self.propagate_constants_to_globals(module);

        // 4. Constant propagation to code (linear trace)
        changed |= self.propagate_constants_to_code(module);

        changed || more
    }

    fn fold_single_uses<'a>(
        &self,
        module: &mut Module<'a>,
        infos: &HashMap<crate::ir::GlobalId, GlobalInfo>,
    ) -> bool {
        let mut folder = SingleUseFolder {
            globals: &module.globals,
            infos,
            changed: false,
            allocator: module.allocator,
        };
        for (_, func) in &mut module.functions {
            if let Some(mut body) = func.body() {
                folder.visit(&mut body);
                *func.body_mut() = Some(body);
            }
        }
        folder.changed
    }

    fn remove_unneeded_writes<'a>(
        &self,
        module: &Module<'a>,
        infos: &HashMap<crate::ir::GlobalId, GlobalInfo>,
    ) -> (bool, HashSet<crate::ir::GlobalId>) {
        let mut to_remove_sets = HashSet::new();
        let mut progress = false;

        for (id, global) in module.globals.iter() {
            let info = &infos[&id.cast()];
            // If it's only written in read_only_to_write patterns and never read otherwise
            if global.mutable
                && !info.exported
                && info.read == info.read_only_to_write
                && info.read > 0
            {
                to_remove_sets.insert(id.cast());
                progress = true;
            }
        }

        (progress, to_remove_sets)
    }

    fn apply_set_removal<'a>(
        &self,
        module: &mut Module<'a>,
        to_remove: HashSet<crate::ir::GlobalId>,
    ) {
        let mut remover = GlobalSetRemover {
            to_remove,
            builder: IrBuilder::new(module.allocator),
        };
        for (_, func) in &mut module.functions {
            if let Some(mut body) = func.body() {
                remover.visit(&mut body);
                *func.body_mut() = Some(body);
            }
        }
    }

    fn propagate_constants_to_globals<'a>(&self, module: &mut Module<'a>) -> bool {
        let mut constants = HashMap::new();
        for (id, global) in module.globals.iter() {
            if !global.mutable {
                if let Some(init) = global.kind_to_init() {
                    if let ExpressionKind::Const(lit) = &init.kind {
                        constants.insert(id.cast(), *lit);
                    }
                }
            }
        }
        if constants.is_empty() {
            return false;
        }

        let mut changed = false;
        let builder = IrBuilder::new(module.allocator);
        let ids: Vec<_> = module.globals.iter().map(|(id, _)| id).collect();
        for id in ids {
            let mut applier = GlobalConstantApplier {
                constants: &constants,
                builder: &builder,
                changed: false,
            };
            if let Some(mut init) = module.globals[id].kind_to_init() {
                applier.visit(&mut init);
                // Re-wrap init back into GlobalKind
                module.globals[id].kind.set_init(init);
                if applier.changed {
                    changed = true;
                }
            }
        }
        changed
    }

    fn propagate_constants_to_code<'a>(&self, module: &mut Module<'a>) -> bool {
        let mut constant_globals = HashSet::new();
        for (id, global) in module.globals.iter() {
            if !global.mutable {
                if let Some(init) = global.kind_to_init() {
                    if let ExpressionKind::Const(_) = &init.kind {
                        constant_globals.insert(id.cast());
                    }
                }
            }
        }

        let mut changed = false;
        // Optimization: if no changed possible, skip
        if constant_globals.is_empty() {
            // We still check for linear trace even if no globals are constant
        }

        {
            let globals = &module.globals;
            let allocator = module.allocator;
            for (_, func) in &mut module.functions {
                if let Some(mut body) = func.body() {
                    let mut applier = ConstantCodeApplier {
                        globals,
                        constant_globals: &constant_globals,
                        curr_constants: HashMap::new(),
                        builder: IrBuilder::new(allocator),
                        replaced: false,
                    };
                    applier.visit(&mut body);
                    *func.body_mut() = Some(body);
                    if applier.replaced {
                        changed = true;
                    }
                }
            }
        }
        changed
    }
}

impl Pass for SimplifyGlobals {
    fn name(&self) -> &str {
        "simplify-globals"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        // We limit iterations for safety, though the algorithm should converge.
        for _ in 0..100 {
            if !self.iteration(module) {
                break;
            }
        }
    }
}

#[derive(Default, Clone)]
struct GlobalInfo {
    exported: bool,
    written: usize,
    read: usize,
    read_only_to_write: usize,
}

struct GlobalUsageTracker<'a> {
    module: &'a Module<'a>,
    infos: HashMap<crate::ir::GlobalId, GlobalInfo>,
}

impl<'a> GlobalUsageTracker<'a> {
    fn new(module: &'a Module<'a>) -> Self {
        let mut infos = HashMap::new();
        for (id, _) in module.globals.iter() {
            infos.insert(id.cast(), GlobalInfo::default());
        }
        for export in &module.exports {
            if let ExportKind::Global(id) = export.kind {
                if let Some(info) = infos.get_mut(&id) {
                    info.exported = true;
                }
            }
        }
        Self { module, infos }
    }

    fn track(&mut self) {
        for (_, func) in &self.module.functions {
            if let Some(body) = func.body() {
                self.visit_usage(body);
            }
        }
        for (_, global) in &self.module.globals {
            if let Some(init) = global.kind_to_init() {
                self.visit_usage(init);
            }
        }
    }

    fn visit_usage(&mut self, expr: ExprRef<'a>) {
        match &expr.kind {
            ExpressionKind::GlobalGet { id } => {
                if let Some(info) = self.infos.get_mut(id) {
                    info.read += 1;
                }
            }
            ExpressionKind::GlobalSet { id, value } => {
                if let Some(info) = self.infos.get_mut(id) {
                    info.written += 1;
                }
                self.visit_usage(*value);
            }
            ExpressionKind::If {
                condition,
                if_true,
                if_false,
                ..
            } => {
                if if_false.is_none() {
                    if let Some(global_idx) = self.is_read_only_to_write(*condition, *if_true) {
                        if let Some(info) = self.infos.get_mut(&global_idx) {
                            info.read_only_to_write += 1;
                        }
                    }
                }
                self.visit_usage(*condition);
                self.visit_usage(*if_true);
                if let Some(f) = if_false {
                    self.visit_usage(*f);
                }
            }
            _ => {
                expr.kind.for_each_child(|child| self.visit_usage(child));
            }
        }
    }

    fn is_read_only_to_write(
        &self,
        condition: ExprRef<'a>,
        code: ExprRef<'a>,
    ) -> Option<crate::ir::GlobalId> {
        if let ExpressionKind::GlobalSet { id, value } = &code.kind {
            if let ExpressionKind::Const(_) = &value.kind {
                if self.expression_reads_global(condition, *id) {
                    return Some(*id);
                }
            }
        }
        None
    }

    fn expression_reads_global(&self, expr: ExprRef<'a>, target_id: crate::ir::GlobalId) -> bool {
        if let ExpressionKind::GlobalGet { id } = &expr.kind {
            if *id == target_id {
                return true;
            }
        }
        let mut reads = false;
        expr.kind.for_each_child(|child| {
            if self.expression_reads_global(child, target_id) {
                reads = true;
            }
        });
        reads
    }
}

struct SingleUseFolder<'a, 'b, 'c> {
    globals: &'b crate::collections::id_arena::IdArena<
        crate::ir::module::GlobalMarker,
        crate::ir::module::Global<'a>,
    >,
    infos: &'c HashMap<crate::ir::GlobalId, GlobalInfo>,
    changed: bool,
    allocator: &'a bumpalo::Bump,
}

impl<'a, 'b, 'c> Visitor<'a> for SingleUseFolder<'a, 'b, 'c> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        if let ExpressionKind::GlobalGet { id } = &expr.kind {
            if let Some(info) = self.infos.get(id) {
                if info.written == 0 && info.read == 1 && !info.exported {
                    let global = &self.globals[id.cast()];
                    if !global.is_import() {
                        if let ExpressionKind::Const(lit) = &global.kind_to_init().unwrap().kind {
                            *expr = IrBuilder::new(self.allocator).const_(*lit);
                            self.changed = true;
                            return;
                        }
                    }
                }
            }
        }
        self.visit_children(expr);
    }
}

struct GlobalSetRemover<'a> {
    to_remove: HashSet<crate::ir::GlobalId>,
    builder: IrBuilder<'a>,
}

impl<'a> Visitor<'a> for GlobalSetRemover<'a> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        if let ExpressionKind::GlobalSet { id, value } = &expr.kind {
            if self.to_remove.contains(id) {
                *expr = self.builder.drop(*value);
            }
        }
        self.visit_children(expr);
    }
}

struct GlobalConstantApplier<'a, 'b> {
    constants: &'b HashMap<crate::ir::GlobalId, Literal>,
    builder: &'b IrBuilder<'a>,
    changed: bool,
}

impl<'a, 'b> Visitor<'a> for GlobalConstantApplier<'a, 'b> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        if let ExpressionKind::GlobalGet { id } = &expr.kind {
            if let Some(lit) = self.constants.get(id) {
                *expr = self.builder.const_(*lit);
                self.changed = true;
            }
        }
        self.visit_children(expr);
    }
}

struct ConstantCodeApplier<'a, 'b, 'c> {
    globals: &'b crate::collections::id_arena::IdArena<
        crate::ir::module::GlobalMarker,
        crate::ir::module::Global<'a>,
    >,
    constant_globals: &'c HashSet<crate::ir::GlobalId>,
    curr_constants: HashMap<crate::ir::GlobalId, Literal>,
    builder: IrBuilder<'a>,
    replaced: bool,
}

impl<'a, 'b, 'c> Visitor<'a> for ConstantCodeApplier<'a, 'b, 'c> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        // Handle GlobalSet for linear trace
        let mut set_idx = None;
        if let ExpressionKind::GlobalSet { id, .. } = &expr.kind {
            set_idx = Some(*id);
        }

        if let Some(id) = set_idx {
            self.visit_children(expr);
            if let ExpressionKind::GlobalSet { value, .. } = &expr.kind {
                if let ExpressionKind::Const(lit) = &value.kind {
                    self.curr_constants.insert(id, *lit);
                } else {
                    self.curr_constants.remove(&id);
                }
            }
            return;
        }

        // Handle GlobalGet
        if let ExpressionKind::GlobalGet { id } = &expr.kind {
            // 1. Check if it's a constant global
            if self.constant_globals.contains(id) {
                let global = &self.globals[id.cast()];
                if !global.is_import() {
                    if let ExpressionKind::Const(lit) = &global.kind_to_init().unwrap().kind {
                        *expr = self.builder.const_(*lit);
                        self.replaced = true;
                        return;
                    }
                }
            }
            // 2. Check linear trace
            if let Some(lit) = self.curr_constants.get(id) {
                *expr = self.builder.const_(*lit);
                self.replaced = true;
                return;
            }
        }

        match &expr.kind {
            ExpressionKind::Call { .. } | ExpressionKind::CallIndirect { .. } => {
                self.curr_constants.clear();
            }
            _ => {}
        }
        self.visit_children(expr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::module::{Function, Global, Module};
    use crate::ir::FuncId;
    use crate::ir::{Literal, Type};
    use bumpalo::Bump;

    fn setup_module<'a>(bump: &'a Bump) -> (Module<'a>, IrBuilder<'a>) {
        (Module::new(bump), IrBuilder::new(bump))
    }

    #[test]
    fn test_propagate_constant_global() {
        let bump = Bump::new();
        let (mut module, builder) = setup_module(&bump);
        let g0 = module.add_global(Global {
            name: "g0".to_string(),
            type_: Type::I32,
            mutable: false,
            kind: crate::ir::module::GlobalKind::Local {
                init: builder.const_(Literal::I32(42)),
            },
        });
        let body = builder.global_get(g0, Type::I32);
        module.functions.push(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        ));
        SimplifyGlobals::default().run(&mut module);
        let body = module.functions[FuncId::from_raw_parts(0, 0)]
            .body()
            .unwrap();
        if let ExpressionKind::Const(Literal::I32(42)) = &body.kind {
            return;
        }
        panic!("Failed to propagate constant");
    }

    #[test]
    fn test_linear_trace_propagation() {
        let bump = Bump::new();
        let (mut module, builder) = setup_module(&bump);
        let g0 = module.add_global(Global {
            name: "g0".to_string(),
            type_: Type::I32,
            mutable: true,
            kind: crate::ir::module::GlobalKind::Local {
                init: builder.const_(Literal::I32(0)),
            },
        });
        let set = builder.global_set(g0, builder.const_(Literal::I32(100)));
        let get = builder.global_get(g0, Type::I32);
        let ret = builder.return_(Some(get));

        let mut list = bumpalo::collections::Vec::new_in(&bump);
        list.push(set);
        list.push(ret);
        let block = builder.block(None, list, Type::I32);

        module.functions.push(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(block),
        ));
        SimplifyGlobals::default().run(&mut module);
        let body = module.functions[FuncId::from_raw_parts(0, 0)]
            .body()
            .unwrap();
        if let ExpressionKind::Block { list, .. } = &body.kind {
            if let ExpressionKind::Return { value: Some(val) } = &list[1].kind {
                if let ExpressionKind::Const(Literal::I32(100)) = &val.kind {
                    return;
                }
            }
        }
        panic!("Failed to propagate through linear trace");
    }

    #[test]
    fn test_read_only_to_write_removal() {
        let bump = Bump::new();
        let (mut module, builder) = setup_module(&bump);
        // Mutable global, initially 0
        let g0 = module.add_global(Global {
            name: "g0".to_string(),
            type_: Type::I32,
            mutable: true,
            kind: crate::ir::module::GlobalKind::Local {
                init: builder.const_(Literal::I32(0)),
            },
        });

        // if (g0 == 0) g0 = 1;
        let cond = builder.binary(
            crate::ir::ops::BinaryOp::EqInt32,
            builder.global_get(g0, Type::I32),
            builder.const_(Literal::I32(0)),
            Type::I32,
        );
        let set = builder.global_set(g0, builder.const_(Literal::I32(1)));
        let if_ = builder.if_(cond, set, None, Type::NONE);

        module.functions.push(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(if_),
        ));

        // This global is only read to be compared with a constant and then written a constant.
        // It's never exported and never read otherwise.
        // It should be removed because it's "read-only-to-write" (logic from C++ SimplifyGlobals).
        SimplifyGlobals::default().run(&mut module);

        let body = module.functions[FuncId::from_raw_parts(0, 0)]
            .body()
            .unwrap();
        // The GlobalSet should be replaced by a Drop(1) or similar if removed.
        if let ExpressionKind::If { if_true, .. } = &body.kind {
            if let ExpressionKind::Drop { value } = &if_true.kind {
                if let ExpressionKind::Const(Literal::I32(1)) = &value.kind {
                    return;
                }
            }
        }
        panic!("Failed to remove read-only-to-write global");
    }
}
