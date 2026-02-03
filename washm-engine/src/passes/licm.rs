use crate::analysis::effects::{Effect, EffectAnalyzer};
use crate::collections::id_arena::IdArena;
use crate::ir::expression::{
    Expression, ExpressionId, ExpressionKind, ExpressionMarker, IrBuilder,
};
use crate::ir::module::{FuncId, Module};
use crate::ir::visitor::{ReadOnlyVisitor, Visitor};
use crate::passes::Pass;
use std::collections::HashSet;

pub struct LICM;

impl Pass for LICM {
    fn name(&self) -> &str {
        "licm"
    }

    fn run(&mut self, module: &mut Module) {
        let func_ids: Vec<_> = module.functions.iter().map(|(id, _)| id).collect();
        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();

        for func_id in func_ids {
            let func = &mut functions[func_id];
            if let Some(mut body) = func.body() {
                let mut hoister = LoopInvariantHoister::new(func_id, func);
                hoister.visit(&mut body, arena);

                if let Some(body_mut) = functions[func_id].body_mut() {
                    *body_mut = body;
                }
            }
        }
    }
}

struct LoopInfo {
    effects: Effect,
    modified_locals: HashSet<crate::ir::LocalId>,
}

struct LoopInvariantHoister<'a> {
    _func_id: FuncId,
    func: &'a mut crate::ir::module::Function,
    loop_stack: Vec<LoopInfo>,
}

impl<'a> LoopInvariantHoister<'a> {
    fn new(_func_id: FuncId, func: &'a mut crate::ir::module::Function) -> Self {
        Self {
            _func_id,
            func,
            loop_stack: Vec::new(),
        }
    }
}

struct LocalReadCollector {
    read_locals: HashSet<crate::ir::LocalId>,
}

impl Default for LocalReadCollector {
    fn default() -> Self {
        Self {
            read_locals: HashSet::new(),
        }
    }
}

impl ReadOnlyVisitor for LocalReadCollector {
    fn visit_expression(
        &mut self,
        expr_id: ExpressionId,
        arena: &IdArena<ExpressionMarker, Expression>,
    ) {
        if let ExpressionKind::LocalGet { id } = &arena[expr_id].kind {
            self.read_locals.insert(*id);
        }
    }
}

impl<'a> Visitor for LoopInvariantHoister<'a> {
    fn visit(
        &mut self,
        expr: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        // First visit children in case there are nested loops
        self.visit_children(expr, arena);
        self.visit_expression(expr, arena);
    }

    fn visit_expression(
        &mut self,
        expr_id: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        if let ExpressionKind::Loop { body, .. } = arena[*expr_id].kind {
            let mut body_copy = body;

            // Analyze loop
            let effects = EffectAnalyzer::analyze(*expr_id, arena);

            // Find all local.set in loop
            let mut finder = LocalWriteFinder::default();
            finder.visit(*expr_id, arena);
            let modified_locals = finder.writes;

            let loop_info = LoopInfo {
                effects,
                modified_locals,
            };

            self.loop_stack.push(loop_info);

            let hoisted_list = {
                // Hoist invariants
                let mut replacer = InvariantReplacer {
                    func: self.func,
                    loop_info: self.loop_stack.last().unwrap(),
                    hoisted: Vec::new(),
                };

                // We need to visit the BODY of the loop
                replacer.visit(&mut body_copy, arena);
                replacer.hoisted
            };

            self.loop_stack.pop();

            // Re-update body in loop if it changed
            if let ExpressionKind::Loop {
                body: ref mut b, ..
            } = arena[*expr_id].kind
            {
                *b = body_copy;
            }

            // If we hoisted anything, wrap loop in a block
            if !hoisted_list.is_empty() {
                let mut builder = IrBuilder::new(arena);
                let mut list = hoisted_list;
                list.push(*expr_id);
                let loop_type = builder.arena[*expr_id].type_;
                *expr_id = builder.block(None::<String>, list, loop_type);
            }
        }
    }
}

struct LocalWriteFinder {
    writes: HashSet<crate::ir::LocalId>,
}

impl Default for LocalWriteFinder {
    fn default() -> Self {
        Self {
            writes: HashSet::new(),
        }
    }
}

impl ReadOnlyVisitor for LocalWriteFinder {
    fn visit_expression(
        &mut self,
        expr_id: ExpressionId,
        arena: &IdArena<ExpressionMarker, Expression>,
    ) {
        match &arena[expr_id].kind {
            ExpressionKind::LocalSet { id, .. } | ExpressionKind::LocalTee { id, .. } => {
                self.writes.insert(*id);
            }
            _ => {}
        }
    }
}

struct InvariantReplacer<'a, 'b> {
    func: &'a mut crate::ir::module::Function,
    loop_info: &'b LoopInfo,
    hoisted: Vec<ExpressionId>,
}

impl<'a, 'b> InvariantReplacer<'a, 'b> {
    fn is_invariant(
        &self,
        expr_id: ExpressionId,
        arena: &IdArena<ExpressionMarker, Expression>,
    ) -> bool {
        let effects = EffectAnalyzer::analyze(expr_id, arena);

        // No side effects allowed
        if effects.intersects(Effect::SIDE_EFFECTS | Effect::MAY_TRAP) {
            return false;
        }

        // Check memory/global interference
        if effects.contains(Effect::MEMORY_READ)
            && self.loop_info.effects.contains(Effect::MEMORY_WRITE)
        {
            return false;
        }
        if effects.contains(Effect::GLOBAL_READ)
            && self.loop_info.effects.contains(Effect::GLOBAL_WRITE)
        {
            return false;
        }

        // Check local read/write interference
        let mut reader = LocalReadCollector::default();
        reader.visit(expr_id, arena);
        for local in reader.read_locals {
            if self.loop_info.modified_locals.contains(&local) {
                return false;
            }
        }

        true
    }

    fn is_trivial(
        &self,
        expr_id: ExpressionId,
        arena: &IdArena<ExpressionMarker, Expression>,
    ) -> bool {
        match &arena[expr_id].kind {
            ExpressionKind::Nop | ExpressionKind::Const(_) | ExpressionKind::Unreachable => true,
            ExpressionKind::LocalGet { .. } | ExpressionKind::GlobalGet { .. } => true,
            _ => false,
        }
    }
}

impl<'a, 'b> Visitor for InvariantReplacer<'a, 'b> {
    fn visit_expression(
        &mut self,
        expr_id: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        // Don't hoist trivial expressions (const, local.get)
        if self.is_trivial(*expr_id, arena) {
            return;
        }

        if self.is_invariant(*expr_id, arena) {
            // Hoist: replace with local.get, add local.set to hoisted list
            let expr_type = arena[*expr_id].type_;
            let original_expr = *expr_id;

            let new_local = self.func.locals_mut().alloc(crate::ir::module::Local {
                name: String::new(),
                type_: expr_type,
            });

            let mut builder = IrBuilder::new(arena);
            self.hoisted
                .push(builder.local_set(new_local, original_expr));
            *expr_id = builder.local_get(new_local, expr_type);
        }
    }
}
