use crate::analysis::effects::{Effect, EffectAnalyzer};
use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::module::Module;
use crate::ir::Type;
use crate::passes::re_finalize::ReFinalize;
use crate::passes::Pass;
use std::collections::HashMap;

/// SimplifyLocals pass - implements local-related optimizations
pub struct SimplifyLocals {
    allow_tee: bool,
    allow_structure: bool,
    allow_nesting: bool,
    another_cycle: bool,
    refinalize: bool,
    path: std::collections::HashSet<ExpressionId>,
}

impl SimplifyLocals {
    /// Create with all optimizations enabled
    pub fn new() -> Self {
        Self {
            allow_tee: true,
            allow_structure: true,
            allow_nesting: true,
            another_cycle: false,
            refinalize: false,
            path: std::collections::HashSet::new(),
        }
    }

    /// Create with custom options
    pub fn with_options(allow_tee: bool, allow_structure: bool, allow_nesting: bool) -> Self {
        Self {
            allow_tee,
            allow_structure,
            allow_nesting,
            another_cycle: false,
            refinalize: false,
            path: std::collections::HashSet::new(),
        }
    }

    /// Create with "flat" mode - no nesting allowed
    pub fn flat() -> Self {
        Self {
            allow_tee: false,
            allow_structure: false,
            allow_nesting: false,
            another_cycle: false,
            refinalize: false,
            path: std::collections::HashSet::new(),
        }
    }
}

impl Default for SimplifyLocals {
    fn default() -> Self {
        Self::new()
    }
}

/// Information about a sinkable local.set
#[derive(Clone, PartialEq)]
struct SinkableInfo {
    /// Effect analysis of the set operation
    effects: Effect,
    /// The local.set expression ID
    set_id: ExpressionId,
    /// The ID of the Expression at the original location (for Nop-ing)
    original_id: ExpressionId,
}

/// Information about an exit from a block
struct BlockBreak {
    /// The break expression ID (br, br_if)
    br_id: ExpressionId,
    /// Sinkables at the point of the break
    sinkables: HashMap<crate::ir::LocalId, SinkableInfo>,
}

/// Context for a single function optimization
struct FunctionContext {
    /// Map from local index to sinkable info
    sinkables: HashMap<crate::ir::LocalId, SinkableInfo>,
    /// Count of local.get operations per local
    get_counts: HashMap<crate::ir::LocalId, usize>,
    /// Whether this is the first optimization cycle
    first_cycle: bool,
    /// Options
    allow_tee: bool,
    _allow_structure: bool,
    _allow_nesting: bool,
    /// Sinkable traces that exit blocks
    block_breaks: HashMap<ExpressionId, Vec<BlockBreak>>,
    /// Blocks that cannot produce return values
    unoptimizable_blocks: std::collections::HashSet<ExpressionId>,
    /// Stack of sinkables for if-else branches
    if_stack: Vec<HashMap<crate::ir::LocalId, SinkableInfo>>,
    /// Whether we need a refinalize
    another_cycle: bool,
}

impl FunctionContext {
    fn new(allow_tee: bool, allow_structure: bool, allow_nesting: bool) -> Self {
        Self {
            sinkables: HashMap::new(),
            get_counts: HashMap::new(),
            first_cycle: true,
            allow_tee,
            _allow_structure: allow_structure,
            _allow_nesting: allow_nesting,
            block_breaks: HashMap::new(),
            unoptimizable_blocks: std::collections::HashSet::new(),
            if_stack: Vec::new(),
            another_cycle: false,
        }
    }

    /// Check if a local.set can be sunk
    fn can_sink(
        &mut self,
        expr: &Expression,
        _arena: &IdArena<ExpressionMarker, Expression>,
    ) -> bool {
        if let ExpressionKind::LocalSet { id, .. } = &expr.kind {
            let use_count = self.get_counts.get(id).copied().unwrap_or(0);
            if use_count > 1 {
                if !self.allow_tee {
                    return false;
                }
                if self.first_cycle {
                    return false;
                }
            }
            true
        } else {
            false
        }
    }

    /// Check if effects invalidate any sinkables
    fn check_invalidations(&mut self, effects: Effect) {
        self.sinkables
            .retain(|_, info| !effects.interferes_with(info.effects));
    }
}

impl Pass for SimplifyLocals {
    fn name(&self) -> &str {
        "SimplifyLocals"
    }

    fn run(&mut self, module: &mut Module) {
        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        for (_, func) in functions {
            if let Some(body) = func.body_mut() {
                let mut ctx =
                    FunctionContext::new(self.allow_tee, self.allow_structure, self.allow_nesting);

                // Multiple cycles until convergence
                loop {
                    self.another_cycle = false;
                    self.refinalize = false;
                    ctx.another_cycle = false;
                    ctx.sinkables.clear();
                    ctx.get_counts.clear();
                    ctx.block_breaks.clear();
                    ctx.unoptimizable_blocks.clear();
                    ctx.if_stack.clear();

                    count_gets(*body, arena, &mut ctx.get_counts);

                    self.optimize_function(body, &mut ctx, arena, 0);

                    if self.refinalize {
                        ReFinalize::run(body, arena);
                    }

                    if !self.another_cycle && !ctx.another_cycle {
                        break;
                    }
                    ctx.first_cycle = false;
                }
            }
        }
    }
}

impl SimplifyLocals {
    fn optimize_function(
        &mut self,
        expr_id: &mut ExpressionId,
        ctx: &mut FunctionContext,
        arena: &mut IdArena<ExpressionMarker, Expression>,
        depth: usize,
    ) {
        if !self.path.insert(*expr_id) {
            return;
        }
        if depth > 1000 {
            self.path.remove(expr_id);
            return;
        }

        self.optimize_function_internal(expr_id, ctx, arena, depth);

        self.path.remove(expr_id);
    }

    fn optimize_function_internal(
        &mut self,
        expr_id: &mut ExpressionId,
        ctx: &mut FunctionContext,
        arena: &mut IdArena<ExpressionMarker, Expression>,
        depth: usize,
    ) {
        let kind = arena[*expr_id].kind.clone();
        match kind {
            ExpressionKind::LocalGet { id } => {
                if let Some(info) = ctx.sinkables.remove(&id) {
                    let set_kind = arena[info.set_id].kind.clone();
                    if let ExpressionKind::LocalSet { value, .. } = set_kind {
                        let value_type = arena[value].type_;
                        let expr_type = arena[*expr_id].type_;
                        if ctx.first_cycle || ctx.get_counts.get(&id).copied().unwrap_or(0) == 1 {
                            if value_type != expr_type {
                                self.refinalize = true;
                            }
                            *expr_id = value;
                        } else {
                            arena[*expr_id].kind = ExpressionKind::LocalTee { id, value };
                        }

                        arena[info.original_id].kind = ExpressionKind::Nop;
                        arena[info.original_id].type_ = Type::NONE;
                        self.another_cycle = true;
                    }
                    let effects = EffectAnalyzer::analyze(*expr_id, arena);
                    ctx.check_invalidations(effects);
                } else {
                    let effects = EffectAnalyzer::analyze(*expr_id, arena);
                    ctx.check_invalidations(effects);
                }
            }
            ExpressionKind::LocalSet { id, mut value } => {
                self.optimize_function(&mut value, ctx, arena, depth + 1);
                if let ExpressionKind::LocalSet { value: old_val, .. } = &mut arena[*expr_id].kind {
                    *old_val = value;
                }
                let effects = EffectAnalyzer::analyze(*expr_id, arena);
                ctx.check_invalidations(effects);

                if ctx.can_sink(&arena[*expr_id], arena) {
                    ctx.sinkables.insert(
                        id,
                        SinkableInfo {
                            set_id: *expr_id,
                            original_id: *expr_id,
                            effects,
                        },
                    );
                } else if ctx.first_cycle && ctx.allow_tee {
                    let use_count = ctx.get_counts.get(&id).copied().unwrap_or(0);
                    if use_count > 1 {
                        self.another_cycle = true;
                    }
                }
            }
            ExpressionKind::LocalTee { id: _, mut value } => {
                self.optimize_function(&mut value, ctx, arena, depth + 1);
                if let ExpressionKind::LocalTee { value: old_val, .. } = &mut arena[*expr_id].kind {
                    *old_val = value;
                }
                let effects = EffectAnalyzer::analyze(*expr_id, arena);
                ctx.check_invalidations(effects);
            }
            ExpressionKind::Drop { mut value } => {
                self.optimize_function(&mut value, ctx, arena, depth + 1);
                if let ExpressionKind::Drop { value: old_val } = &mut arena[*expr_id].kind {
                    *old_val = value;
                }
                self.visit_drop_post(expr_id, ctx, arena);
                let effects = EffectAnalyzer::analyze(*expr_id, arena);
                ctx.check_invalidations(effects);
            }
            ExpressionKind::Block { mut list, .. } => {
                for child in list.iter_mut() {
                    self.optimize_function(child, ctx, arena, depth + 1);
                }
                if let ExpressionKind::Block { list: old_list, .. } = &mut arena[*expr_id].kind {
                    *old_list = list;
                }
                self.visit_block_post(expr_id, ctx, arena);
            }
            ExpressionKind::If {
                name: _,
                mut condition,
                mut if_true,
                mut if_false,
            } => {
                self.optimize_function(&mut condition, ctx, arena, depth + 1);

                let snapshot = ctx.sinkables.clone();
                self.optimize_function(&mut if_true, ctx, arena, depth + 1);

                if let Some(mut fb) = if_false {
                    let true_sinkables = std::mem::replace(&mut ctx.sinkables, snapshot);
                    ctx.if_stack.push(true_sinkables);
                    self.optimize_function(&mut fb, ctx, arena, depth + 1);
                    if_false = Some(fb);
                } else {
                    let true_sinkables = std::mem::replace(&mut ctx.sinkables, snapshot);
                    ctx.if_stack.push(true_sinkables);
                }

                if let ExpressionKind::If {
                    name: _,
                    condition: oc,
                    if_true: ot,
                    if_false: of,
                } = &mut arena[*expr_id].kind
                {
                    *oc = condition;
                    *ot = if_true;
                    *of = if_false;
                }
                self.visit_if_post(expr_id, ctx, arena);
            }
            ExpressionKind::Loop { mut body, .. } => {
                let snapshot = ctx.sinkables.clone();
                ctx.sinkables.clear();
                self.optimize_function(&mut body, ctx, arena, depth + 1);
                if let ExpressionKind::Loop { body: ob, .. } = &mut arena[*expr_id].kind {
                    *ob = body;
                }
                self.visit_loop_post(expr_id, ctx, arena);
                ctx.sinkables = snapshot;
            }
            ExpressionKind::Break { target, value, .. } => {
                let target_id = target;
                if value.is_some() {
                    ctx.unoptimizable_blocks.insert(target_id);
                } else {
                    ctx.block_breaks
                        .entry(target_id)
                        .or_default()
                        .push(BlockBreak {
                            br_id: *expr_id,
                            sinkables: ctx.sinkables.clone(),
                        });
                }
                ctx.sinkables.clear();
            }
            _ => {
                let mut kind_mut = arena[*expr_id].kind.clone();
                kind_mut.for_each_child_mut(|child| {
                    self.optimize_function(child, ctx, arena, depth + 1);
                });
                arena[*expr_id].kind = kind_mut;
                let effects = EffectAnalyzer::analyze(*expr_id, arena);
                ctx.check_invalidations(effects);
            }
        }
    }

    fn visit_block_post(
        &mut self,
        expr_id: &mut ExpressionId,
        ctx: &mut FunctionContext,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        if !matches!(arena[*expr_id].kind, ExpressionKind::Block { .. }) {
            return;
        }

        let id = *expr_id;
        let hb = ctx.block_breaks.contains_key(&id);

        if self.allow_structure {
            self.optimize_block_return(*expr_id, ctx, arena);
        }

        if ctx.unoptimizable_blocks.contains(&id) {
            ctx.sinkables.clear();
            ctx.unoptimizable_blocks.remove(&id);
        }
        if hb {
            ctx.sinkables.clear();
            ctx.block_breaks.remove(&id);
        }

        ReFinalize::run(expr_id, arena);
    }

    fn optimize_block_return(
        &mut self,
        expr_id: ExpressionId,
        ctx: &mut FunctionContext,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        let list_len = match &arena[expr_id].kind {
            ExpressionKind::Block { list, .. } => list.len(),
            _ => return,
        };

        if ctx.unoptimizable_blocks.contains(&expr_id) {
            return;
        }

        let breaks = ctx.block_breaks.get(&expr_id);

        if breaks.is_none() || breaks.as_ref().unwrap().is_empty() {
            let mut result = None;
            if let ExpressionKind::Block { list, .. } = &arena[expr_id].kind {
                if let Some(&last_id) = list.last() {
                    if let ExpressionKind::LocalSet { id, .. } = &arena[last_id].kind {
                        result = Some((*id, last_id));
                    }
                }
            }

            if let Some((id, last_set_id)) = result {
                let val_id =
                    if let ExpressionKind::LocalSet { value, .. } = &arena[last_set_id].kind {
                        *value
                    } else {
                        unreachable!()
                    };

                if let ExpressionKind::Block { list, .. } = &mut arena[expr_id].kind {
                    if let Some(last) = list.last_mut() {
                        *last = val_id;
                    }
                }

                let old_kind = std::mem::replace(&mut arena[expr_id].kind, ExpressionKind::Nop);
                let old_type = arena[expr_id].type_;

                arena[last_set_id].kind = old_kind;
                arena[last_set_id].type_ = old_type;

                arena[expr_id].kind = ExpressionKind::LocalSet {
                    id,
                    value: last_set_id,
                };
                arena[expr_id].type_ = Type::NONE;

                self.refinalize = true;
                self.another_cycle = true;
            }
            return;
        }

        let breaks = breaks.unwrap();
        let mut shared_id = None;
        for &id in ctx.sinkables.keys() {
            let mut in_all = true;
            for brk in breaks {
                if !brk.sinkables.contains_key(&id) {
                    in_all = false;
                    break;
                }
            }
            if in_all {
                shared_id = Some(id);
                break;
            }
        }

        let shared_id = match shared_id {
            Some(i) => i,
            None => return,
        };

        for brk in breaks {
            if let ExpressionKind::Break {
                condition: Some(cond),
                ..
            } = &arena[brk.br_id].kind
            {
                let set_info = brk.sinkables.get(&shared_id).unwrap();
                let cond_effects = EffectAnalyzer::analyze(*cond, arena);
                if cond_effects.interferes_with(set_info.effects) {
                    return;
                }
            }
        }

        if list_len == 0 {
            return;
        }

        let set_info = ctx.sinkables.remove(&shared_id).unwrap();
        let block_val = if let ExpressionKind::LocalSet { value, .. } = &arena[set_info.set_id].kind
        {
            *value
        } else {
            unreachable!()
        };

        if let ExpressionKind::Block { list, .. } = &mut arena[expr_id].kind {
            list.push(block_val);
        }
        arena[set_info.set_id].kind = ExpressionKind::Nop;
        arena[set_info.set_id].type_ = Type::NONE;

        for brk in breaks {
            let info = brk.sinkables.get(&shared_id).unwrap();
            let sid = info.set_id;
            let vid = if let ExpressionKind::LocalSet { value, .. } = &arena[sid].kind {
                *value
            } else {
                unreachable!()
            };

            if let ExpressionKind::Break {
                value, condition, ..
            } = &mut arena[brk.br_id].kind
            {
                if condition.is_some() {
                    *value = Some(sid);
                    arena[sid].kind = ExpressionKind::LocalTee {
                        id: shared_id,
                        value: vid,
                    };
                } else {
                    *value = Some(vid);
                    arena[sid].kind = ExpressionKind::Nop;
                    arena[sid].type_ = Type::NONE;
                }
            }
        }

        let old_kind = std::mem::replace(&mut arena[expr_id].kind, ExpressionKind::Nop);
        let old_type = arena[expr_id].type_;
        let nid = Expression::alloc(arena, old_kind, old_type);

        arena[expr_id].kind = ExpressionKind::LocalSet {
            id: shared_id,
            value: nid,
        };
        arena[expr_id].type_ = Type::NONE;

        ctx.sinkables.clear();
        self.another_cycle = true;
        self.refinalize = true;
    }

    fn visit_if_post(
        &mut self,
        expr_id: &mut ExpressionId,
        ctx: &mut FunctionContext,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        let true_sinkables = match ctx.if_stack.pop() {
            Some(s) => s,
            None => {
                ctx.sinkables.clear();
                return;
            }
        };

        let mut common = HashMap::new();
        for (id, info) in &true_sinkables {
            if let Some(false_info) = ctx.sinkables.get(id) {
                if info.effects == false_info.effects {
                    common.insert(*id, info.clone());
                }
            }
        }

        ctx.sinkables = common;

        if self.allow_structure {
            self.optimize_if_return(expr_id, ctx, arena, true_sinkables);
        }
    }

    fn optimize_if_return(
        &mut self,
        expr_id: &mut ExpressionId,
        ctx: &mut FunctionContext,
        arena: &mut IdArena<ExpressionMarker, Expression>,
        mut if_true_sinkables: HashMap<crate::ir::LocalId, SinkableInfo>,
    ) {
        if arena[*expr_id].type_ != Type::NONE {
            return;
        }

        let (if_true_id, _if_false_opt) = match &arena[*expr_id].kind {
            ExpressionKind::If {
                name: _,
                if_true,
                if_false,
                ..
            } => (*if_true, *if_false),
            _ => return,
        };

        if arena[if_true_id].type_ != Type::NONE {
            return;
        }

        if if_true_sinkables.is_empty() {
            return;
        }

        let good_id = *if_true_sinkables.keys().next().unwrap();
        let info = if_true_sinkables.remove(&good_id).unwrap();

        if let ExpressionKind::LocalSet { value, .. } = &arena[info.set_id].kind {
            let vid = *value;
            let mut is_whole = false;

            let mut true_id = if_true_id;
            if let ExpressionKind::LocalSet { id, value: val } = &mut arena[true_id].kind {
                if *id == good_id {
                    let ov = *val;
                    arena[true_id].kind =
                        std::mem::replace(&mut arena[ov].kind, ExpressionKind::Nop);
                    arena[true_id].type_ = arena[ov].type_;
                    is_whole = true;
                }
            }

            if !is_whole {
                if !matches!(arena[true_id].kind, ExpressionKind::Block { .. }) {
                    let ok = std::mem::replace(&mut arena[true_id].kind, ExpressionKind::Nop);
                    let ot = arena[true_id].type_;
                    let iid = Expression::alloc(arena, ok, ot);
                    arena[true_id].kind = ExpressionKind::Block {
                        name: None,
                        list: vec![iid],
                    };
                }

                if let ExpressionKind::Block { list, .. } = &mut arena[true_id].kind {
                    list.push(vid);
                    arena[info.original_id].kind = ExpressionKind::Nop;
                    arena[info.original_id].type_ = Type::NONE;
                    ReFinalize::run(&mut true_id, arena);
                }
            }

            let ltype = arena[true_id].type_;
            let gid = Expression::alloc(arena, ExpressionKind::LocalGet { id: good_id }, ltype);

            if let ExpressionKind::If { if_false, .. } = &mut arena[*expr_id].kind {
                if if_false.is_none() {
                    *if_false = Some(gid);
                }
            }

            arena[info.original_id].kind = ExpressionKind::Nop;
            arena[info.original_id].type_ = Type::NONE;

            ReFinalize::run(expr_id, arena);

            let ok = std::mem::replace(&mut arena[*expr_id].kind, ExpressionKind::Nop);
            let ot = arena[*expr_id].type_;
            let nid = Expression::alloc(arena, ok, ot);

            arena[*expr_id].kind = ExpressionKind::LocalSet {
                id: good_id,
                value: nid,
            };
            arena[*expr_id].type_ = Type::NONE;

            ctx.sinkables.clear();
            self.another_cycle = true;
        }
    }

    fn visit_loop_post(
        &mut self,
        expr_id: &mut ExpressionId,
        ctx: &mut FunctionContext,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        if !self.allow_structure {
            ReFinalize::run(expr_id, arena);
            return;
        }

        if arena[*expr_id].type_ != Type::NONE {
            ReFinalize::run(expr_id, arena);
            return;
        }

        if ctx.sinkables.is_empty() {
            return;
        }

        let good_id = *ctx.sinkables.keys().next().unwrap();
        let info = ctx.sinkables.remove(&good_id).unwrap();

        let body_id = match &arena[*expr_id].kind {
            ExpressionKind::Loop { body, .. } => *body,
            _ => return,
        };

        let mut optimized = false;
        if let ExpressionKind::LocalSet { id, value } = &mut arena[body_id].kind {
            if *id == good_id {
                let ov = *value;
                arena[body_id].kind = std::mem::replace(&mut arena[ov].kind, ExpressionKind::Nop);
                arena[body_id].type_ = arena[ov].type_;
                optimized = true;
            }
        } else if let ExpressionKind::Block { .. } = &arena[body_id].kind {
            let vid = if let ExpressionKind::LocalSet { value, .. } = &arena[info.set_id].kind {
                Some(*value)
            } else {
                None
            };

            if let Some(vid) = vid {
                let mut mut_body_id = body_id;
                if let ExpressionKind::Block { list, .. } = &mut arena[mut_body_id].kind {
                    list.push(vid);
                    arena[info.original_id].kind = ExpressionKind::Nop;
                    arena[info.original_id].type_ = Type::NONE;
                    ReFinalize::run(&mut mut_body_id, arena);
                    optimized = true;
                }
            }
        }

        if optimized {
            ReFinalize::run(expr_id, arena);
            let ok = std::mem::replace(&mut arena[*expr_id].kind, ExpressionKind::Nop);
            let ot = arena[*expr_id].type_;
            let nid = Expression::alloc(arena, ok, ot);

            arena[*expr_id].kind = ExpressionKind::LocalSet {
                id: good_id,
                value: nid,
            };
            arena[*expr_id].type_ = Type::NONE;
            self.another_cycle = true;
        }
    }

    fn visit_drop_post(
        &mut self,
        expr_id: &mut ExpressionId,
        _ctx: &mut FunctionContext,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        if let ExpressionKind::Drop { value } = &mut arena[*expr_id].kind {
            let vid = *value;
            if let ExpressionKind::LocalTee { id, value: tee_val } = &arena[vid].kind {
                let id = *id;
                let tv = *tee_val;
                arena[*expr_id].kind = ExpressionKind::LocalSet { id, value: tv };
                arena[*expr_id].type_ = Type::NONE;
                self.another_cycle = true;
            }
        }
    }
}

fn count_gets(
    id: ExpressionId,
    arena: &IdArena<ExpressionMarker, Expression>,
    counts: &mut HashMap<crate::ir::LocalId, usize>,
) {
    let kind = &arena[id].kind;
    match kind {
        ExpressionKind::LocalGet { id } => {
            *counts.entry(*id).or_insert(0) += 1;
        }
        _ => {
            kind.for_each_child(|child| count_gets(child, arena, counts));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::IrBuilder;
    use crate::ir::module::{Function, Module};
    use crate::ir::Literal;
    use crate::ir::LocalId;
    use crate::ir::Type;

    #[test]
    fn test_simplify_locals_options() {
        let pass1 = SimplifyLocals::with_options(true, true, true);
        assert!(pass1.allow_tee && pass1.allow_structure && pass1.allow_nesting);

        let pass2 = SimplifyLocals::flat();
        assert!(!pass2.allow_tee && !pass2.allow_structure && !pass2.allow_nesting);
    }

    #[test]
    fn test_sink_to_tee() {
        let mut module = Module::new();
        let mut builder = IrBuilder::new(&mut module.expressions);

        let mut list = Vec::new();
        let c42 = builder.const_(Literal::I32(42));
        list.push(builder.local_set(LocalId::from(0), c42));
        let lget1 = builder.local_get(LocalId::from(0), Type::I32);
        list.push(builder.drop(lget1));
        let lget2 = builder.local_get(LocalId::from(0), Type::I32);
        list.push(builder.drop(lget2));

        let block = builder.block("", list, Type::NONE);
        let test_id = module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![Type::I32],
            block,
        ));

        let mut pass = SimplifyLocals::new();
        pass.run(&mut module);

        let body_id = module.functions.get(test_id).unwrap().body().unwrap();
        if let ExpressionKind::Block { list, .. } = &module.expressions[body_id].kind {
            assert_eq!(list.len(), 3);
            assert!(matches!(
                module.expressions[list[0]].kind,
                ExpressionKind::Nop
            ));
            assert!(matches!(
                module.expressions[list[1]].kind,
                ExpressionKind::Nop
            ));
            if let ExpressionKind::Drop { value } = &module.expressions[list[2]].kind {
                assert!(matches!(
                    module.expressions[*value].kind,
                    ExpressionKind::Const(Literal::I32(42))
                ));
            } else {
                panic!();
            }
        } else {
            panic!();
        }
    }

    #[test]
    fn test_structure_opt() {
        let mut module = Module::new();
        let mut builder = IrBuilder::new(&mut module.expressions);

        let condition = builder.local_get(LocalId::from(1), Type::I32);
        let mut true_list = Vec::new();
        let c1 = builder.const_(Literal::I32(1));
        true_list.push(builder.local_set(LocalId::from(0), c1));
        let if_true = builder.block("", true_list, Type::NONE);

        let mut false_list = Vec::new();
        let c2 = builder.const_(Literal::I32(2));
        false_list.push(builder.local_set(LocalId::from(0), c2));
        let if_false = Some(builder.block("", false_list, Type::NONE));

        let if_expr = builder.if_(None::<&str>, condition, if_true, if_false, Type::NONE);
        let test_id = module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![Type::I32, Type::I32],
            if_expr,
        ));

        let mut pass = SimplifyLocals::new();
        pass.run(&mut module);

        let body_id = module.functions.get(test_id).unwrap().body().unwrap();
        if let ExpressionKind::LocalSet { id, value } = &module.expressions[body_id].kind {
            assert_eq!(id.index() as usize, 0);
            assert!(matches!(
                module.expressions[*value].kind,
                ExpressionKind::If { .. }
            ));
        } else {
            panic!();
        }
    }

    #[test]
    fn test_block_return_opt() {
        let mut module = Module::new();
        let mut builder = IrBuilder::new(&mut module.expressions);

        let mut list = Vec::new();
        let c1 = builder.const_(Literal::I32(1));
        list.push(builder.local_set(LocalId::from(0), c1));
        let block = builder.block(Some("label".to_string()), list, Type::NONE);
        let test_id = module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![Type::I32],
            block,
        ));

        let mut pass = SimplifyLocals::new();
        pass.run(&mut module);

        let body_id = module.functions.get(test_id).unwrap().body().unwrap();
        if let ExpressionKind::LocalSet { id, .. } = &module.expressions[body_id].kind {
            assert_eq!(id.index() as usize, 0);
        } else {
            panic!();
        }
    }

    #[test]
    fn test_if_return_opt() {
        let mut module = Module::new();
        let mut builder = IrBuilder::new(&mut module.expressions);

        let condition = builder.local_get(LocalId::from(1), Type::I32);
        let mut true_list = Vec::new();
        let c1 = builder.const_(Literal::I32(1));
        true_list.push(builder.local_set(LocalId::from(0), c1));
        let if_true = builder.block("", true_list, Type::NONE);
        let if_expr = builder.if_(None::<&str>, condition, if_true, None, Type::NONE);
        let test_id = module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![Type::I32, Type::I32],
            if_expr,
        ));

        let mut pass = SimplifyLocals::new();
        pass.run(&mut module);

        let body_id = module.functions.get(test_id).unwrap().body().unwrap();
        if let ExpressionKind::LocalSet { id, value } = &module.expressions[body_id].kind {
            assert_eq!(id.index() as usize, 0);
            if let ExpressionKind::If { if_false, .. } = &module.expressions[*value].kind {
                assert!(if_false.is_some());
            } else {
                panic!();
            }
        } else {
            panic!();
        }
    }

    #[test]
    fn test_loop_return_opt() {
        let mut module = Module::new();
        let mut builder = IrBuilder::new(&mut module.expressions);

        let mut list = Vec::new();
        let c1 = builder.const_(Literal::I32(1));
        list.push(builder.local_set(LocalId::from(0), c1));
        let block = builder.block("", list, Type::NONE);
        let loop_expr = builder.loop_(Some("label".to_string()), block, Type::NONE);
        let test_id = module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![Type::I32],
            loop_expr,
        ));

        let mut pass = SimplifyLocals::new();
        pass.run(&mut module);

        let body_id = module.functions.get(test_id).unwrap().body().unwrap();
        if let ExpressionKind::LocalSet { id, value } = &module.expressions[body_id].kind {
            assert_eq!(id.index() as usize, 0);
            assert!(matches!(
                module.expressions[*value].kind,
                ExpressionKind::Loop { .. }
            ));
        } else {
            panic!();
        }
    }
}
