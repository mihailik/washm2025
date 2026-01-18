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
        for (idx, global) in module.globals.iter_mut().enumerate() {
            let info = &infos[&idx];
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
        infos: &HashMap<usize, GlobalInfo>,
    ) -> bool {
        let mut folder = SingleUseFolder {
            globals: &module.globals,
            infos,
            changed: false,
            allocator: module.allocator,
        };
        for func in &mut module.functions {
            if let Some(mut body) = func.body {
                folder.visit(&mut body);
                func.body = Some(body);
            }
        }
        folder.changed
    }

    fn remove_unneeded_writes<'a>(
        &self,
        module: &Module<'a>,
        infos: &HashMap<usize, GlobalInfo>,
    ) -> (bool, HashSet<usize>) {
        let mut to_remove_sets = HashSet::new();
        let mut progress = false;

        for (idx, global) in module.globals.iter().enumerate() {
            let info = &infos[&idx];
            // If it's only written in read_only_to_write patterns and never read otherwise
            if global.mutable
                && !info.exported
                && info.read == info.read_only_to_write
                && info.read > 0
            {
                to_remove_sets.insert(idx);
                progress = true;
            }
        }

        (progress, to_remove_sets)
    }

    fn apply_set_removal<'a>(&self, module: &mut Module<'a>, to_remove: HashSet<usize>) {
        let mut remover = GlobalSetRemover {
            to_remove,
            builder: IrBuilder::new(module.allocator),
        };
        for func in &mut module.functions {
            if let Some(mut body) = func.body {
                remover.visit(&mut body);
                func.body = Some(body);
            }
        }
    }

    fn propagate_constants_to_globals<'a>(&self, module: &mut Module<'a>) -> bool {
        let mut constants = HashMap::new();
        for (idx, global) in module.globals.iter().enumerate() {
            if !global.mutable {
                if let ExpressionKind::Const(lit) = &global.init.kind {
                    constants.insert(idx, lit.clone());
                }
            }
        }
        if constants.is_empty() {
            return false;
        }

        let mut changed = false;
        let builder = IrBuilder::new(module.allocator);
        for i in 0..module.globals.len() {
            let mut applier = GlobalConstantApplier {
                constants: &constants,
                builder: &builder,
                changed: false,
            };
            let mut init = module.globals[i].init;
            applier.visit(&mut init);
            module.globals[i].init = init;
            if applier.changed {
                changed = true;
            }
        }
        changed
    }

    fn propagate_constants_to_code<'a>(&self, module: &mut Module<'a>) -> bool {
        let mut constant_globals = HashSet::new();
        for (idx, global) in module.globals.iter().enumerate() {
            if !global.mutable {
                if let ExpressionKind::Const(_) = &global.init.kind {
                    constant_globals.insert(idx);
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
            for func in &mut module.functions {
                if let Some(mut body) = func.body {
                    let mut applier = ConstantCodeApplier {
                        globals,
                        constant_globals: &constant_globals,
                        curr_constants: HashMap::new(),
                        builder: IrBuilder::new(allocator),
                        replaced: false,
                    };
                    applier.visit(&mut body);
                    func.body = Some(body);
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
    infos: HashMap<usize, GlobalInfo>,
}

impl<'a> GlobalUsageTracker<'a> {
    fn new(module: &'a Module<'a>) -> Self {
        let mut infos = HashMap::new();
        for (idx, _) in module.globals.iter().enumerate() {
            infos.insert(idx, GlobalInfo::default());
        }
        for export in &module.exports {
            if export.kind == ExportKind::Global {
                if let Some(info) = infos.get_mut(&(export.index as usize)) {
                    info.exported = true;
                }
            }
        }
        Self { module, infos }
    }

    fn track(&mut self) {
        for func in &self.module.functions {
            if let Some(body) = func.body {
                self.visit_usage(body);
            }
        }
        for global in &self.module.globals {
            self.visit_usage(global.init);
        }
    }

    fn visit_usage(&mut self, expr: ExprRef<'a>) {
        match &expr.kind {
            ExpressionKind::GlobalGet { index } => {
                if let Some(info) = self.infos.get_mut(&(*index as usize)) {
                    info.read += 1;
                }
            }
            ExpressionKind::GlobalSet { index, value } => {
                let idx = *index as usize;
                if let Some(info) = self.infos.get_mut(&idx) {
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

    fn is_read_only_to_write(&self, condition: ExprRef<'a>, code: ExprRef<'a>) -> Option<usize> {
        if let ExpressionKind::GlobalSet { index, value } = &code.kind {
            if let ExpressionKind::Const(_) = &value.kind {
                if self.expression_reads_global(condition, *index as usize) {
                    return Some(*index as usize);
                }
            }
        }
        None
    }

    fn expression_reads_global(&self, expr: ExprRef<'a>, target_idx: usize) -> bool {
        if let ExpressionKind::GlobalGet { index } = &expr.kind {
            if *index as usize == target_idx {
                return true;
            }
        }
        let mut reads = false;
        expr.kind.for_each_child(|child| {
            if self.expression_reads_global(child, target_idx) {
                reads = true;
            }
        });
        reads
    }
}

struct SingleUseFolder<'a, 'b, 'c> {
    globals: &'b [crate::ir::module::Global<'a>],
    infos: &'c HashMap<usize, GlobalInfo>,
    changed: bool,
    allocator: &'a bumpalo::Bump,
}

impl<'a, 'b, 'c> Visitor<'a> for SingleUseFolder<'a, 'b, 'c> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        if let ExpressionKind::GlobalGet { index } = &expr.kind {
            let idx = *index as usize;
            if let Some(info) = self.infos.get(&idx) {
                if info.written == 0 && info.read == 1 && !info.exported {
                    let global = &self.globals[idx];
                    if let ExpressionKind::Const(lit) = &global.init.kind {
                        *expr = IrBuilder::new(self.allocator).const_(lit.clone());
                        self.changed = true;
                        return;
                    }
                }
            }
        }
        self.visit_children(expr);
    }
}

struct GlobalSetRemover<'a> {
    to_remove: HashSet<usize>,
    builder: IrBuilder<'a>,
}

impl<'a> Visitor<'a> for GlobalSetRemover<'a> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        if let ExpressionKind::GlobalSet { index, value } = &expr.kind {
            if self.to_remove.contains(&(*index as usize)) {
                *expr = self.builder.drop(*value);
            }
        }
        self.visit_children(expr);
    }
}

struct GlobalConstantApplier<'a, 'b> {
    constants: &'b HashMap<usize, Literal>,
    builder: &'b IrBuilder<'a>,
    changed: bool,
}

impl<'a, 'b> Visitor<'a> for GlobalConstantApplier<'a, 'b> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        if let ExpressionKind::GlobalGet { index } = &expr.kind {
            if let Some(lit) = self.constants.get(&(*index as usize)) {
                *expr = self.builder.const_(lit.clone());
                self.changed = true;
            }
        }
        self.visit_children(expr);
    }
}

struct ConstantCodeApplier<'a, 'b, 'c> {
    globals: &'b [crate::ir::module::Global<'a>],
    constant_globals: &'c HashSet<usize>,
    curr_constants: HashMap<usize, Literal>,
    builder: IrBuilder<'a>,
    replaced: bool,
}

impl<'a, 'b, 'c> Visitor<'a> for ConstantCodeApplier<'a, 'b, 'c> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        // Handle GlobalSet for linear trace
        let mut set_idx = None;
        if let ExpressionKind::GlobalSet { index, .. } = &expr.kind {
            set_idx = Some(*index as usize);
        }

        if let Some(idx) = set_idx {
            self.visit_children(expr);
            if let ExpressionKind::GlobalSet { value, .. } = &expr.kind {
                if let ExpressionKind::Const(lit) = &value.kind {
                    self.curr_constants.insert(idx, lit.clone());
                } else {
                    self.curr_constants.remove(&idx);
                }
            }
            return;
        }

        // Handle GlobalGet
        if let ExpressionKind::GlobalGet { index } = &expr.kind {
            let idx = *index as usize;
            // 1. Check if it's a constant global
            if self.constant_globals.contains(&idx) {
                let global = &self.globals[idx];
                if let ExpressionKind::Const(lit) = &global.init.kind {
                    *expr = self.builder.const_(lit.clone());
                    self.replaced = true;
                    return;
                }
            }
            // 2. Check linear trace
            if let Some(lit) = self.curr_constants.get(&idx) {
                *expr = self.builder.const_(lit.clone());
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
    use crate::ir::{Literal, Type};
    use bumpalo::Bump;

    fn setup_module<'a>(bump: &'a Bump) -> (Module<'a>, IrBuilder<'a>) {
        (Module::new(bump), IrBuilder::new(bump))
    }

    #[test]
    fn test_propagate_constant_global() {
        let bump = Bump::new();
        let (mut module, builder) = setup_module(&bump);
        module.globals.push(Global {
            name: "g0".to_string(),
            type_: Type::I32,
            mutable: false,
            init: builder.const_(Literal::I32(42)),
        });
        let body = builder.global_get(0, Type::I32);
        module.functions.push(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(body),
        ));
        SimplifyGlobals::default().run(&mut module);
        let body = module.functions[0].body.unwrap();
        if let ExpressionKind::Const(Literal::I32(42)) = &body.kind {
            return;
        }
        panic!("Failed to propagate constant");
    }

    #[test]
    fn test_linear_trace_propagation() {
        let bump = Bump::new();
        let (mut module, builder) = setup_module(&bump);
        module.globals.push(Global {
            name: "g0".to_string(),
            type_: Type::I32,
            mutable: true,
            init: builder.const_(Literal::I32(0)),
        });
        let set = builder.global_set(0, builder.const_(Literal::I32(100)));
        let get = builder.global_get(0, Type::I32);
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
        let body = module.functions[0].body.unwrap();
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
        module.globals.push(Global {
            name: "g0".to_string(),
            type_: Type::I32,
            mutable: true,
            init: builder.const_(Literal::I32(0)),
        });

        // if (g0 == 0) g0 = 1;
        let cond = builder.binary(
            crate::ir::ops::BinaryOp::EqInt32,
            builder.global_get(0, Type::I32),
            builder.const_(Literal::I32(0)),
            Type::I32,
        );
        let set = builder.global_set(0, builder.const_(Literal::I32(1)));
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

        let body = module.functions[0].body.unwrap();
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
