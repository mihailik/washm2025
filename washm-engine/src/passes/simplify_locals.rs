use crate::analysis::effects::{Effect, EffectAnalyzer};
use crate::ir::expression::{ExprRef, Expression, ExpressionKind};
use crate::ir::module::Module;
use crate::passes::Pass;
use bumpalo::collections::Vec as BumpVec;
use std::collections::HashMap;

/// SimplifyLocals pass - implements local-related optimizations
///
/// This pass performs "sinking" optimizations for local.set operations:
/// - Pushes local.set operations closer to their local.get usage
/// - Removes local.set operations if no gets remain
/// - Creates local.tee when a local has multiple uses
/// - Merges local.sets into block/if return values
///
/// Options:
/// - allow_tee: Allow creating local.tee for multiple uses
/// - allow_structure: Create block/if return values by merging internal sets
/// - allow_nesting: Allow sinking that creates nested expressions
pub struct SimplifyLocals {
    allow_tee: bool,
    allow_structure: bool,
    allow_nesting: bool,
    another_cycle: bool,
    path: std::collections::HashSet<usize>,
}

impl SimplifyLocals {
    /// Create with all optimizations enabled
    pub fn new() -> Self {
        Self {
            allow_tee: true,
            allow_structure: true,
            allow_nesting: true,
            another_cycle: false,
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
struct SinkableInfo<'a> {
    /// Effect analysis of the set operation
    effects: Effect,
    /// The local.set expression itself
    set: ExprRef<'a>,
    /// Pointer to the Expression at the original location (for Nop-ing)
    ptr: *mut Expression<'a>,
}

/// Information about an exit from a block
struct BlockBreak<'a> {
    /// The break expression (br, br_if)
    br: *mut Expression<'a>,
    /// Sinkables at the point of the break
    sinkables: HashMap<u32, SinkableInfo<'a>>,
}

/// Context for a single function optimization
struct FunctionContext<'a> {
    /// Map from local index to sinkable info
    sinkables: HashMap<u32, SinkableInfo<'a>>,
    /// Count of local.get operations per local
    get_counts: HashMap<u32, usize>,
    /// Whether this is the first optimization cycle
    first_cycle: bool,
    /// Options
    allow_tee: bool,
    _allow_structure: bool,
    _allow_nesting: bool,
    /// Sinkable traces that exit blocks
    block_breaks: HashMap<String, Vec<BlockBreak<'a>>>,
    /// Blocks that cannot produce return values
    unoptimizable_blocks: std::collections::HashSet<String>,
    /// Stack of sinkables for if-else branches
    if_stack: Vec<HashMap<u32, SinkableInfo<'a>>>,
    /// Whether we need a refinalize
    refinalize: bool,
}

impl<'a> FunctionContext<'a> {
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
            refinalize: false,
        }
    }

    /// Check if a local.set can be sunk
    fn can_sink(&mut self, set: &Expression) -> bool {
        if let ExpressionKind::LocalSet { index, .. } = &set.kind {
            // If in first cycle or not allowing tees, cannot sink if >1 use
            // (would require creating a tee)
            let use_count = self.get_counts.get(index).copied().unwrap_or(0);
            if use_count > 1 {
                if !self.allow_tee {
                    return false;
                }
                if self.first_cycle {
                    // Deferred to next cycle when we can create a tee
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
        self.sinkables.retain(|_, info| {
            // If the new effects invalidate this sinkable, remove it
            !effects.interferes_with(info.effects)
        });
    }
}

impl Pass for SimplifyLocals {
    fn name(&self) -> &str {
        "SimplifyLocals"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        let mut first_cycle = true;
        // Run multiple cycles until no more changes
        let allocator = module.allocator();
        loop {
            self.another_cycle = false;
            self.path.clear();

            for func in &mut module.functions {
                if let Some(body) = &mut func.body {
                    let mut ctx = FunctionContext::new(
                        self.allow_tee,
                        self.allow_structure,
                        self.allow_nesting,
                    );
                    ctx.first_cycle = first_cycle;

                    // First pass: count local.get operations
                    ctx.get_counts.clear();
                    count_gets(body, &mut ctx.get_counts);

                    // Second pass: optimize
                    self.optimize_function(body, &mut ctx, allocator, 0);

                    if ctx.refinalize {
                        body.finalize();
                    }
                }
            }

            first_cycle = false;
            if !self.another_cycle {
                break;
            }
        }
    }
}

impl SimplifyLocals {
    fn optimize_function<'a>(
        &mut self,
        expr: &mut ExprRef<'a>,
        ctx: &mut FunctionContext<'a>,
        allocator: &'a bumpalo::Bump,
        depth: usize,
    ) {
        let addr = expr.as_ptr() as usize;
        if !self.path.insert(addr) {
            panic!("Cycle detected at address: 0x{:x}", addr);
        }
        if depth > 1000 {
            panic!("Recursion depth exceeded!");
        }

        self.optimize_function_internal(expr, ctx, allocator, depth);

        self.path.remove(&addr);
    }

    fn optimize_function_internal<'a>(
        &mut self,
        expr: &mut ExprRef<'a>,
        ctx: &mut FunctionContext<'a>,
        allocator: &'a bumpalo::Bump,
        depth: usize,
    ) {
        // Handle LocalGet
        let get_index = if let ExpressionKind::LocalGet { index } = &expr.kind {
            Some(*index)
        } else {
            None
        };
        if let Some(index) = get_index {
            // Sinking logic
            if let Some(info) = ctx.sinkables.remove(&index) {
                unsafe {
                    let set_kind = &mut (*info.set.as_ptr()).kind;
                    if let ExpressionKind::LocalSet { value, .. } = set_kind {
                        if ctx.first_cycle || ctx.get_counts.get(&index).copied().unwrap_or(0) == 1
                        {
                            // Single use: Replace Get with Value
                            if (*value).type_ != expr.type_ {
                                ctx.refinalize = true;
                            }
                            *expr = *value;
                        } else {
                            // Multiple uses: Replace Get with LocalTee
                            expr.kind = ExpressionKind::LocalTee {
                                index,
                                value: *value,
                            };
                        }

                        // Nop out the Set at its original location
                        let old_set_ref = &mut *info.ptr;
                        old_set_ref.kind = ExpressionKind::Nop;
                        old_set_ref.type_ = crate::ir::Type::NONE;

                        self.another_cycle = true;
                    } else {
                        unreachable!("Sinkable set must be a LocalSet");
                    }
                }
            }
            let effects = EffectAnalyzer::analyze(*expr);
            ctx.check_invalidations(effects);
            return;
        }

        // Handle LocalSet
        let set_index = if let ExpressionKind::LocalSet { index, .. } = &expr.kind {
            Some(*index)
        } else {
            None
        };

        if let Some(index) = set_index {
            {
                if let ExpressionKind::LocalSet { value, .. } = &mut expr.kind {
                    self.optimize_function(value, ctx, allocator, depth + 1);
                }
            }

            let effects = EffectAnalyzer::analyze(*expr);
            ctx.check_invalidations(effects);

            if ctx.can_sink(&*expr) {
                ctx.sinkables.insert(
                    index,
                    SinkableInfo {
                        set: *expr,
                        ptr: expr.as_ptr(),
                        effects,
                    },
                );
            } else if ctx.first_cycle && ctx.allow_tee {
                let use_count = ctx.get_counts.get(&index).copied().unwrap_or(0);
                if use_count > 1 {
                    self.another_cycle = true;
                }
            }
            return;
        }
        let tee_vals = if let ExpressionKind::LocalTee { index, .. } = &expr.kind {
            Some(*index)
        } else {
            None
        };
        if tee_vals.is_some() {
            if let ExpressionKind::LocalTee { value, .. } = &mut expr.kind {
                self.optimize_function(value, ctx, allocator, depth + 1);
            }
            let effects = EffectAnalyzer::analyze(*expr);
            ctx.check_invalidations(effects);
            return;
        }

        // Handle Drop
        if let ExpressionKind::Drop { value } = &mut expr.kind {
            self.optimize_function(value, ctx, allocator, depth + 1);
            self.visit_drop_post(expr, ctx);

            let effects = EffectAnalyzer::analyze(*expr);
            ctx.check_invalidations(effects);
            return;
        }

        // Handle Control Flow and Others
        match &mut expr.kind {
            ExpressionKind::Block { list, .. } => {
                for child in list.iter_mut() {
                    self.optimize_function(child, ctx, allocator, depth + 1);
                }
                self.visit_block_post(expr, ctx, allocator);
            }
            ExpressionKind::If {
                condition,
                if_true,
                if_false,
            } => {
                self.optimize_function(condition, ctx, allocator, depth + 1);

                let snapshot = ctx.sinkables.clone();
                self.optimize_function(if_true, ctx, allocator, depth + 1);

                if let Some(false_branch) = if_false {
                    // Save true branch sinkables
                    let true_sinkables = std::mem::replace(&mut ctx.sinkables, snapshot);
                    ctx.if_stack.push(true_sinkables);

                    self.optimize_function(false_branch, ctx, allocator, depth + 1);

                    // Now ctx.sinkables has false branch sinkables, and if_stack has true branch
                } else {
                    let true_sinkables = std::mem::replace(&mut ctx.sinkables, snapshot);
                    ctx.if_stack.push(true_sinkables);
                    // ctx.sinkables is now back to 'snapshot' (empty or earlier sinkables)
                }

                self.visit_if_post(expr, ctx, allocator);
            }
            ExpressionKind::Loop { body, .. } => {
                let snapshot = ctx.sinkables.clone();
                ctx.sinkables.clear();
                self.optimize_function(body, ctx, allocator, depth + 1);
                self.visit_loop_post(expr, ctx, allocator);
                ctx.sinkables = snapshot;
            }
            ExpressionKind::LocalGet { .. }
            | ExpressionKind::LocalSet { .. }
            | ExpressionKind::LocalTee { .. }
            | ExpressionKind::Drop { .. } => {
                unreachable!("Handled above")
            }
            ExpressionKind::Break { name, value, .. } => {
                let target = *name;
                if value.is_some() {
                    ctx.unoptimizable_blocks.insert(target.to_string());
                } else {
                    ctx.block_breaks
                        .entry(target.to_string())
                        .or_default()
                        .push(BlockBreak {
                            br: expr.as_ptr(),
                            sinkables: ctx.sinkables.clone(),
                        });
                }
                ctx.sinkables.clear();
            }
            _ => {
                visit_children(expr, |child| {
                    self.optimize_function(child, ctx, allocator, depth + 1)
                });
                expr.finalize();
                let effects = EffectAnalyzer::analyze(*expr);
                ctx.check_invalidations(effects);
            }
        }
    }

    fn visit_block_post<'a>(
        &mut self,
        expr: &mut ExprRef<'a>,
        ctx: &mut FunctionContext<'a>,
        allocator: &'a bumpalo::Bump,
    ) {
        let (name, has_breaks) = if let ExpressionKind::Block { name, .. } = &expr.kind {
            let n = name.map(|s| s.to_string());
            let hb = n
                .as_ref()
                .map(|s| ctx.block_breaks.contains_key(s))
                .unwrap_or(false);
            (n, hb)
        } else {
            return;
        };

        if self.allow_structure {
            self.optimize_block_return(expr, ctx, allocator);
        }

        if let Some(n) = name {
            if ctx.unoptimizable_blocks.contains(&n) {
                ctx.sinkables.clear();
                ctx.unoptimizable_blocks.remove(&n);
            }
            if has_breaks {
                ctx.sinkables.clear();
                ctx.block_breaks.remove(&n);
            }
        }
        expr.finalize();
    }

    fn optimize_block_return<'a>(
        &mut self,
        expr: &mut ExprRef<'a>,
        ctx: &mut FunctionContext<'a>,
        allocator: &'a bumpalo::Bump,
    ) {
        let (name, list) = match &mut expr.kind {
            ExpressionKind::Block { name, list } => (name, list),
            _ => return,
        };

        if let Some(n) = name {
            if ctx.unoptimizable_blocks.contains(*n) {
                return;
            }
        }

        let breaks = if let Some(n) = name {
            ctx.block_breaks.get(*n)
        } else {
            None
        };

        // If no breaks, we can still try to optimize the end of the block
        if breaks.is_none() || breaks.unwrap().is_empty() {
            let mut result = None;
            if let ExpressionKind::Block { list, .. } = &mut expr.kind {
                if let Some(last) = list.last_mut() {
                    if let ExpressionKind::LocalSet { index, .. } = &last.kind {
                        result = Some((*index, *last));
                    }
                }
            }

            if let Some((index, last_set)) = result {
                let value = if let ExpressionKind::LocalSet { value, .. } = &last_set.kind {
                    *value
                } else {
                    unreachable!()
                };

                let new_block_type = value.type_;
                if let ExpressionKind::Block { list, .. } = &mut expr.kind {
                    *list.last_mut().unwrap() = value;
                }

                let old_block_kind = std::mem::replace(&mut expr.kind, ExpressionKind::Nop);
                unsafe {
                    let ptr = last_set.as_ptr();
                    (*ptr).kind = old_block_kind;
                    (*ptr).type_ = new_block_type;
                }

                expr.kind = ExpressionKind::LocalSet {
                    index,
                    value: last_set,
                };
                self.another_cycle = true;
            }
            return;
        }

        let breaks = breaks.unwrap();

        // Find a local set that is present in all breaks and the current sinkables (if any)
        let mut shared_index = None;
        for &index in ctx.sinkables.keys() {
            let mut in_all = true;
            for brk in breaks {
                if !brk.sinkables.contains_key(&index) {
                    in_all = false;
                    break;
                }
            }
            if in_all {
                shared_index = Some(index);
                break;
            }
        }

        let shared_index = match shared_index {
            Some(i) => i,
            None => return,
        };

        // Verify if any br_if condition invalidates the move
        for brk in breaks {
            unsafe {
                if let ExpressionKind::Break {
                    condition: Some(cond),
                    ..
                } = &(*brk.br).kind
                {
                    let set_info = brk.sinkables.get(&shared_index).unwrap();
                    let cond_effects = EffectAnalyzer::analyze(*cond);
                    if cond_effects.interferes_with(set_info.effects) {
                        return;
                    }
                }
            }
        }

        // Optimization possible!
        if list.is_empty() {
            return;
        }

        // 1. End of block
        let set_info = ctx.sinkables.remove(&shared_index).unwrap();
        let block_val = if let ExpressionKind::LocalSet { value, .. } = &set_info.set.kind {
            *value
        } else {
            unreachable!()
        };

        list.push(block_val);
        unsafe {
            let set_ref = &mut *set_info.ptr;
            set_ref.kind = ExpressionKind::Nop;
            set_ref.type_ = crate::ir::Type::NONE;
        }

        // 2. Each break
        for brk in breaks {
            unsafe {
                let info = brk.sinkables.get(&shared_index).unwrap();
                let set = info.set;
                if let ExpressionKind::Break {
                    condition, value, ..
                } = &mut (*brk.br).kind
                {
                    if let ExpressionKind::LocalSet { value: val, .. } = &set.kind {
                        if condition.is_some() {
                            *value = Some(set);
                            // Convert Set to Tee
                            let set_raw_ptr = set.as_ptr();
                            (*set_raw_ptr).kind = ExpressionKind::LocalTee {
                                index: shared_index,
                                value: *val,
                            };
                            let info_ptr_ref = &mut *info.ptr;
                            info_ptr_ref.kind = ExpressionKind::Nop;
                            info_ptr_ref.type_ = crate::ir::Type::NONE;
                        } else {
                            *value = Some(*val);
                            let info_ptr_ref = &mut *info.ptr;
                            info_ptr_ref.kind = ExpressionKind::Nop;
                            info_ptr_ref.type_ = crate::ir::Type::NONE;
                        }
                    }
                }
            }
        }

        expr.finalize();

        // Correctly wrap the block in a LocalSet by moving its kind to a new expression
        let old_kind = std::mem::replace(&mut expr.kind, ExpressionKind::Nop);
        let old_type = expr.type_;
        let new_block_expr = Expression::new(allocator, old_kind, old_type);

        expr.kind = ExpressionKind::LocalSet {
            index: shared_index,
            value: new_block_expr,
        };
        ctx.sinkables.clear();
        self.another_cycle = true;
    }

    fn visit_if_post<'a>(
        &mut self,
        expr: &mut ExprRef<'a>,
        ctx: &mut FunctionContext<'a>,
        allocator: &'a bumpalo::Bump,
    ) {
        if !self.allow_structure {
            return;
        }

        let is_if_else = if let ExpressionKind::If { if_false, .. } = &expr.kind {
            if_false.is_some()
        } else {
            false
        };

        if is_if_else {
            let true_sinkables = ctx.if_stack.pop().unwrap();
            self.optimize_if_else_return(expr, ctx, allocator, true_sinkables);
        } else {
            let true_sinkables = ctx.if_stack.pop().unwrap();
            self.optimize_if_return(expr, ctx, allocator, true_sinkables);
        }
        expr.finalize();
    }

    fn optimize_if_else_return<'a>(
        &mut self,
        expr: &mut ExprRef<'a>,
        ctx: &mut FunctionContext<'a>,
        allocator: &'a bumpalo::Bump,
        mut if_true_sinkables: HashMap<u32, SinkableInfo<'a>>,
    ) {
        if expr.type_ != crate::ir::Type::NONE {
            return;
        }

        let (mut if_true, if_false) = if let ExpressionKind::If {
            if_true, if_false, ..
        } = &mut expr.kind
        {
            (*if_true, if_false.as_mut().unwrap())
        } else {
            return;
        };
        let mut if_false = *if_false;

        let mut good_index = None;
        if if_true.type_ == crate::ir::Type::UNREACHABLE {
            if !ctx.sinkables.is_empty() {
                good_index = Some(*ctx.sinkables.keys().next().unwrap());
            }
        } else if if_false.type_ == crate::ir::Type::UNREACHABLE {
            if !if_true_sinkables.is_empty() {
                good_index = Some(*if_true_sinkables.keys().next().unwrap());
            }
        } else {
            // Shared index
            for &index in if_true_sinkables.keys() {
                if ctx.sinkables.contains_key(&index) {
                    good_index = Some(index);
                    break;
                }
            }
        }

        let good_index = match good_index {
            Some(i) => i,
            None => return,
        };

        let mut optimized = false;
        let true_type = if_true.type_;
        if true_type != crate::ir::Type::UNREACHABLE {
            let info = if_true_sinkables.remove(&good_index).unwrap();

            let mut is_whole_branch = false;
            if let ExpressionKind::LocalSet { index, value } = &mut if_true.kind {
                if *index == good_index {
                    let mut old_value = *value;
                    if_true.kind = std::mem::replace(&mut old_value.kind, ExpressionKind::Nop);
                    if_true.type_ = old_value.type_;
                    optimized = true;
                    is_whole_branch = true;
                }
            }

            if !is_whole_branch {
                // Ensure it's a block to append to it
                if !matches!(if_true.kind, ExpressionKind::Block { .. }) {
                    let mut list = BumpVec::new_in(allocator);
                    let old_true = std::mem::replace(&mut if_true.kind, ExpressionKind::Nop);
                    let old_type = if_true.type_;
                    let inner = Expression::new(allocator, old_true, old_type);
                    list.push(inner);
                    if_true.kind = ExpressionKind::Block { name: None, list };
                }

                if let ExpressionKind::LocalSet { value, .. } = &info.set.kind {
                    let value = *value;
                    if let ExpressionKind::Block { list, .. } = &mut if_true.kind {
                        list.push(value);
                        unsafe {
                            let info_ptr_ref = &mut *info.ptr;
                            info_ptr_ref.kind = ExpressionKind::Nop;
                            info_ptr_ref.type_ = crate::ir::Type::NONE;
                        }
                        if_true.finalize();
                        optimized = true;
                    }
                }
            }
        }

        let false_type = if_false.type_;
        if false_type != crate::ir::Type::UNREACHABLE {
            let info = ctx.sinkables.remove(&good_index).unwrap();

            let mut is_whole_branch = false;
            if let ExpressionKind::LocalSet { index, value } = &mut if_false.kind {
                if *index == good_index {
                    let mut old_value = *value;
                    if_false.kind = std::mem::replace(&mut old_value.kind, ExpressionKind::Nop);
                    if_false.type_ = old_value.type_;
                    optimized = true;
                    is_whole_branch = true;
                }
            }

            if !is_whole_branch {
                // Ensure it's a block to append to it
                if !matches!(if_false.kind, ExpressionKind::Block { .. }) {
                    let mut list = BumpVec::new_in(allocator);
                    let old_false = std::mem::replace(&mut if_false.kind, ExpressionKind::Nop);
                    let old_type = if_false.type_;
                    let inner = Expression::new(allocator, old_false, old_type);
                    list.push(inner);
                    if_false.kind = ExpressionKind::Block { name: None, list };
                }

                if let ExpressionKind::LocalSet { value, .. } = &info.set.kind {
                    let value = *value;
                    if let ExpressionKind::Block { list, .. } = &mut if_false.kind {
                        list.push(value);
                        unsafe {
                            let info_ptr_ref = &mut *info.ptr;
                            info_ptr_ref.kind = ExpressionKind::Nop;
                            info_ptr_ref.type_ = crate::ir::Type::NONE;
                        }
                        if_false.finalize();
                        optimized = true;
                    }
                }
            }
        }

        if !optimized {
            return;
        }

        expr.finalize(); // Update If type before wrapping
        let old_kind = std::mem::replace(&mut expr.kind, ExpressionKind::Nop);
        let old_type = expr.type_;
        let new_if = Expression::new(allocator, old_kind, old_type);

        expr.kind = ExpressionKind::LocalSet {
            index: good_index,
            value: new_if,
        };
        expr.finalize();
        ctx.sinkables.clear();
        self.another_cycle = true;
    }

    fn optimize_if_return<'a>(
        &mut self,
        expr: &mut ExprRef<'a>,
        _ctx: &mut FunctionContext<'a>,
        allocator: &'a bumpalo::Bump,
        mut if_true_sinkables: HashMap<u32, SinkableInfo<'a>>,
    ) {
        if expr.type_ != crate::ir::Type::NONE {
            return;
        }

        let if_true = match &mut expr.kind {
            ExpressionKind::If { if_true, .. } => if_true,
            _ => return,
        };

        if if_true.type_ != crate::ir::Type::NONE {
            return;
        }

        if if_true_sinkables.is_empty() {
            return;
        }

        // Speculative: add local.get to else branch to enable sinking
        let good_index = *if_true_sinkables.keys().next().unwrap();
        let info = if_true_sinkables.remove(&good_index).unwrap();

        if let ExpressionKind::LocalSet { value, .. } = &info.set.kind {
            let value = *value;
            let mut is_whole_branch = false;
            if let ExpressionKind::LocalSet { index, value: val } = &mut if_true.kind {
                if *index == good_index {
                    let mut old_val = *val;
                    if_true.kind = std::mem::replace(&mut old_val.kind, ExpressionKind::Nop);
                    if_true.type_ = old_val.type_;
                    is_whole_branch = true;
                }
            }

            if !is_whole_branch {
                if !matches!(if_true.kind, ExpressionKind::Block { .. }) {
                    let mut list = BumpVec::new_in(allocator);
                    let old_true = std::mem::replace(&mut if_true.kind, ExpressionKind::Nop);
                    let old_type = if_true.type_;
                    let inner = Expression::new(allocator, old_true, old_type);
                    list.push(inner);
                    if_true.kind = ExpressionKind::Block { name: None, list };
                }

                if let ExpressionKind::Block { list, .. } = &mut if_true.kind {
                    list.push(value);
                    unsafe {
                        let info_ptr_ref = &mut *info.ptr;
                        info_ptr_ref.kind = ExpressionKind::Nop;
                        info_ptr_ref.type_ = crate::ir::Type::NONE;
                    }
                    if_true.finalize();
                }
            }
            // Speculative: Add the missing else with a local.get
            let local_type = if_true.type_; // Use the type of the value being sunk
            let get = Expression::local_get(allocator, good_index, local_type);

            if let ExpressionKind::If { if_false, .. } = &mut expr.kind {
                *if_false = Some(get);
            }

            unsafe {
                let info_ptr_ref = &mut *info.ptr;
                info_ptr_ref.kind = ExpressionKind::Nop;
                info_ptr_ref.type_ = crate::ir::Type::NONE;
            }

            expr.finalize(); // Update If type before wrapping
            let old_kind = std::mem::replace(&mut expr.kind, ExpressionKind::Nop);
            let old_type = expr.type_;
            let new_if = Expression::new(allocator, old_kind, old_type);

            expr.kind = ExpressionKind::LocalSet {
                index: good_index,
                value: new_if,
            };
            expr.finalize();
            self.another_cycle = true;
        }
    }

    fn visit_loop_post<'a>(
        &mut self,
        expr: &mut ExprRef<'a>,
        ctx: &mut FunctionContext<'a>,
        allocator: &'a bumpalo::Bump,
    ) {
        if !self.allow_structure {
            expr.finalize();
            return;
        }

        if expr.type_ != crate::ir::Type::NONE {
            expr.finalize();
            return;
        }

        if ctx.sinkables.is_empty() {
            return;
        }

        let good_index = *ctx.sinkables.keys().next().unwrap();
        let set_info = ctx.sinkables.remove(&good_index).unwrap();

        if let ExpressionKind::Loop { body, .. } = &mut expr.kind {
            let mut optimized = false;

            if let ExpressionKind::LocalSet { index, value } = &mut body.kind {
                if *index == good_index {
                    let mut old_value = *value;
                    body.kind = std::mem::replace(&mut old_value.kind, ExpressionKind::Nop);
                    body.type_ = old_value.type_;
                    optimized = true;
                }
            } else if let ExpressionKind::Block { list, .. } = &mut body.kind {
                if let ExpressionKind::LocalSet { value, .. } = &set_info.set.kind {
                    let value = *value;
                    list.push(value);
                    unsafe {
                        let set_info_ptr_ref = &mut *set_info.ptr;
                        set_info_ptr_ref.kind = ExpressionKind::Nop;
                        set_info_ptr_ref.type_ = crate::ir::Type::NONE;
                    }
                    body.finalize();
                    optimized = true;
                }
            }

            if optimized {
                expr.finalize(); // Update Loop type before wrapping
                let old_kind = std::mem::replace(&mut expr.kind, ExpressionKind::Nop);
                let old_type = expr.type_;
                let new_loop = Expression::new(allocator, old_kind, old_type);

                expr.kind = ExpressionKind::LocalSet {
                    index: good_index,
                    value: new_loop,
                };
                expr.finalize();
                self.another_cycle = true;
            }
        }
    }

    fn visit_drop_post<'a>(&mut self, expr: &mut ExprRef<'a>, _ctx: &mut FunctionContext<'a>) {
        // Collapse drop-tee into set (drop (local.tee) -> local.set)
        if let ExpressionKind::Drop { value } = &mut expr.kind {
            if let ExpressionKind::LocalTee {
                index,
                value: tee_val,
            } = &value.kind
            {
                let index = *index;
                let tee_val = *tee_val;
                expr.kind = ExpressionKind::LocalSet {
                    index,
                    value: tee_val,
                };
                self.another_cycle = true;
            }
        }
    }
}

/// Count local.get operations in an expression tree
fn count_gets(expr: &Expression, counts: &mut HashMap<u32, usize>) {
    match &expr.kind {
        ExpressionKind::LocalGet { index } => {
            *counts.entry(*index).or_insert(0) += 1;
        }
        _ => {
            visit_children_ref(expr, |child| count_gets(child, counts));
        }
    }
}

/// Visit children of an expression (mutable)
fn visit_children<'a, F>(expr: &mut ExprRef<'a>, mut f: F)
where
    F: FnMut(&mut ExprRef<'a>),
{
    match &mut expr.kind {
        ExpressionKind::Block { list, .. } => {
            for child in list.iter_mut() {
                f(child);
            }
        }
        ExpressionKind::If {
            condition,
            if_true,
            if_false,
        } => {
            f(condition);
            f(if_true);
            if let Some(else_expr) = if_false {
                f(else_expr);
            }
        }
        ExpressionKind::Loop { body, .. } => {
            f(body);
        }
        ExpressionKind::Unary { value, .. } => {
            f(value);
        }
        ExpressionKind::Binary { left, right, .. } => {
            f(left);
            f(right);
        }
        ExpressionKind::Call { operands, .. } | ExpressionKind::CallIndirect { operands, .. } => {
            for operand in operands.iter_mut() {
                f(operand);
            }
            if let ExpressionKind::CallIndirect { target, .. } = &mut expr.kind {
                f(target);
            }
        }
        ExpressionKind::LocalSet { value, .. } | ExpressionKind::LocalTee { value, .. } => {
            f(value);
        }
        ExpressionKind::GlobalSet { value, .. } => {
            f(value);
        }
        ExpressionKind::Drop { value } => {
            f(value);
        }
        ExpressionKind::Break {
            condition, value, ..
        } => {
            if let Some(cond) = condition {
                f(cond);
            }
            if let Some(val) = value {
                f(val);
            }
        }
        ExpressionKind::Return { value } => {
            if let Some(val) = value {
                f(val);
            }
        }
        ExpressionKind::Store { ptr, value, .. } => {
            f(ptr);
            f(value);
        }
        ExpressionKind::Load { ptr, .. } => {
            f(ptr);
        }
        ExpressionKind::AtomicRMW { ptr, value, .. } => {
            f(ptr);
            f(value);
        }
        ExpressionKind::AtomicCmpxchg {
            ptr,
            expected,
            replacement,
            ..
        } => {
            f(ptr);
            f(expected);
            f(replacement);
        }
        ExpressionKind::AtomicWait {
            ptr,
            expected,
            timeout,
            ..
        } => {
            f(ptr);
            f(expected);
            f(timeout);
        }
        ExpressionKind::AtomicNotify { ptr, count, .. } => {
            f(ptr);
            f(count);
        }
        ExpressionKind::Switch {
            condition, value, ..
        } => {
            f(condition);
            if let Some(val) = value {
                f(val);
            }
        }
        ExpressionKind::Select {
            condition,
            if_true,
            if_false,
        } => {
            f(condition);
            f(if_true);
            f(if_false);
        }
        ExpressionKind::MemoryGrow { delta } => {
            f(delta);
        }
        ExpressionKind::RefIsNull { value } => {
            f(value);
        }
        ExpressionKind::RefAs { value, .. } => {
            f(value);
        }
        ExpressionKind::RefEq { left, right } => {
            f(left);
            f(right);
        }
        ExpressionKind::TupleMake { operands } => {
            for operand in operands.iter_mut() {
                f(operand);
            }
        }
        ExpressionKind::TupleExtract { tuple, .. } => {
            f(tuple);
        }
        ExpressionKind::SIMDExtract { vec, .. } => f(vec),
        ExpressionKind::SIMDReplace { vec, value, .. } => {
            f(vec);
            f(value);
        }
        ExpressionKind::SIMDShuffle { left, right, .. } => {
            f(left);
            f(right);
        }
        ExpressionKind::SIMDTernary { a, b, c, .. } => {
            f(a);
            f(b);
            f(c);
        }
        ExpressionKind::SIMDShift { vec, shift, .. } => {
            f(vec);
            f(shift);
        }
        ExpressionKind::SIMDLoad { ptr, .. } => f(ptr),
        ExpressionKind::SIMDLoadStoreLane { ptr, vec, .. } => {
            f(ptr);
            f(vec);
        }
        ExpressionKind::MemoryInit {
            dest, offset, size, ..
        } => {
            f(dest);
            f(offset);
            f(size);
        }
        ExpressionKind::MemoryCopy {
            dest, src, size, ..
        } => {
            f(dest);
            f(src);
            f(size);
        }
        ExpressionKind::MemoryFill {
            dest, value, size, ..
        } => {
            f(dest);
            f(value);
            f(size);
        }
        ExpressionKind::TableGet { index, .. } => f(index),
        ExpressionKind::TableSet { index, value, .. } => {
            f(index);
            f(value);
        }
        ExpressionKind::TableGrow { delta, value, .. } => {
            f(delta);
            f(value);
        }
        ExpressionKind::TableFill {
            dest, value, size, ..
        } => {
            f(dest);
            f(value);
            f(size);
        }
        ExpressionKind::TableCopy {
            dest, src, size, ..
        } => {
            f(dest);
            f(src);
            f(size);
        }
        ExpressionKind::TableInit {
            dest, offset, size, ..
        } => {
            f(dest);
            f(offset);
            f(size);
        }
        ExpressionKind::StructNew { operands, .. } => {
            for operand in operands.iter_mut() {
                f(operand);
            }
        }
        ExpressionKind::StructGet { ptr, .. } => f(ptr),
        ExpressionKind::StructSet { ptr, value, .. } => {
            f(ptr);
            f(value);
        }
        ExpressionKind::ArrayNew { size, init, .. } => {
            f(size);
            if let Some(i) = init {
                f(i);
            }
        }
        ExpressionKind::ArrayGet { ptr, index, .. } => {
            f(ptr);
            f(index);
        }
        ExpressionKind::ArraySet {
            ptr, index, value, ..
        } => {
            f(ptr);
            f(index);
            f(value);
        }
        ExpressionKind::ArrayLen { ptr } => f(ptr),
        ExpressionKind::Try {
            body, catch_bodies, ..
        } => {
            f(body);
            for cb in catch_bodies.iter_mut() {
                f(cb);
            }
        }
        ExpressionKind::Throw { operands, .. } => {
            for op in operands.iter_mut() {
                f(op);
            }
        }
        ExpressionKind::I31New { value } => {
            f(value);
        }
        ExpressionKind::I31Get { i31, .. } => {
            f(i31);
        }
        ExpressionKind::Nop
        | ExpressionKind::Unreachable
        | ExpressionKind::Const(_)
        | ExpressionKind::LocalGet { .. }
        | ExpressionKind::GlobalGet { .. }
        | ExpressionKind::MemorySize
        | ExpressionKind::TableSize { .. }
        | ExpressionKind::AtomicFence
        | ExpressionKind::RefNull { .. }
        | ExpressionKind::RefFunc { .. }
        | ExpressionKind::DataDrop { .. }
        | ExpressionKind::ElemDrop { .. }
        | ExpressionKind::Rethrow { .. }
        | ExpressionKind::Pop { .. }
        | ExpressionKind::RefTest { .. }
        | ExpressionKind::RefCast { .. }
        | ExpressionKind::BrOn { .. } => {}
    }
}

/// Visit children of an expression (immutable)
fn visit_children_ref<F>(expr: &Expression, mut f: F)
where
    F: FnMut(&Expression),
{
    match &expr.kind {
        ExpressionKind::Block { list, .. } => {
            for child in list.iter() {
                f(child);
            }
        }
        ExpressionKind::If {
            condition,
            if_true,
            if_false,
        } => {
            f(condition);
            f(if_true);
            if let Some(else_expr) = if_false {
                f(else_expr);
            }
        }
        ExpressionKind::Loop { body, .. } => {
            f(body);
        }
        ExpressionKind::Binary { left, right, .. } => {
            f(left);
            f(right);
        }
        ExpressionKind::Unary { value, .. } => {
            f(value);
        }
        ExpressionKind::LocalSet { value, .. } | ExpressionKind::LocalTee { value, .. } => {
            f(value);
        }
        ExpressionKind::GlobalSet { value, .. } => {
            f(value);
        }
        ExpressionKind::Drop { value } => {
            f(value);
        }
        ExpressionKind::Break {
            condition, value, ..
        } => {
            if let Some(cond) = condition {
                f(cond);
            }
            if let Some(val) = value {
                f(val);
            }
        }
        ExpressionKind::Return { value: Some(v) } => {
            f(v);
        }
        ExpressionKind::Return { value: None } => {}
        ExpressionKind::Load { ptr, .. } => {
            f(ptr);
        }
        ExpressionKind::Store { ptr, value, .. } => {
            f(ptr);
            f(value);
        }
        ExpressionKind::Call { operands, .. } | ExpressionKind::CallIndirect { operands, .. } => {
            for operand in operands.iter() {
                f(operand);
            }
            if let ExpressionKind::CallIndirect { target, .. } = &expr.kind {
                f(target);
            }
        }
        ExpressionKind::Switch {
            condition, value, ..
        } => {
            f(condition);
            if let Some(v) = value {
                f(v);
            }
        }
        ExpressionKind::Select {
            condition,
            if_true,
            if_false,
        } => {
            f(condition);
            f(if_true);
            f(if_false);
        }
        ExpressionKind::MemoryGrow { delta } => {
            f(delta);
        }
        ExpressionKind::AtomicRMW { ptr, value, .. } => {
            f(ptr);
            f(value);
        }
        ExpressionKind::AtomicCmpxchg {
            ptr,
            expected,
            replacement,
            ..
        } => {
            f(ptr);
            f(expected);
            f(replacement);
        }
        ExpressionKind::AtomicWait {
            ptr,
            expected,
            timeout,
            ..
        } => {
            f(ptr);
            f(expected);
            f(timeout);
        }
        ExpressionKind::AtomicNotify { ptr, count, .. } => {
            f(ptr);
            f(count);
        }
        ExpressionKind::RefIsNull { value } => {
            f(value);
        }
        ExpressionKind::RefAs { value, .. } => {
            f(value);
        }
        ExpressionKind::RefEq { left, right } => {
            f(left);
            f(right);
        }
        ExpressionKind::TupleMake { operands } => {
            for operand in operands.iter() {
                f(operand);
            }
        }
        ExpressionKind::TupleExtract { tuple, .. } => {
            f(tuple);
        }
        ExpressionKind::SIMDExtract { vec, .. } => f(vec),
        ExpressionKind::SIMDReplace { vec, value, .. } => {
            f(vec);
            f(value);
        }
        ExpressionKind::SIMDShuffle { left, right, .. } => {
            f(left);
            f(right);
        }
        ExpressionKind::SIMDTernary { a, b, c, .. } => {
            f(a);
            f(b);
            f(c);
        }
        ExpressionKind::SIMDShift { vec, shift, .. } => {
            f(vec);
            f(shift);
        }
        ExpressionKind::SIMDLoad { ptr, .. } => f(ptr),
        ExpressionKind::SIMDLoadStoreLane { ptr, vec, .. } => {
            f(ptr);
            f(vec);
        }
        ExpressionKind::MemoryInit {
            dest, offset, size, ..
        } => {
            f(dest);
            f(offset);
            f(size);
        }
        ExpressionKind::MemoryCopy {
            dest, src, size, ..
        } => {
            f(dest);
            f(src);
            f(size);
        }
        ExpressionKind::MemoryFill {
            dest, value, size, ..
        } => {
            f(dest);
            f(value);
            f(size);
        }
        ExpressionKind::TableGet { index, .. } => f(index),
        ExpressionKind::TableSet { index, value, .. } => {
            f(index);
            f(value);
        }
        ExpressionKind::TableGrow { delta, value, .. } => {
            f(delta);
            f(value);
        }
        ExpressionKind::TableFill {
            dest, value, size, ..
        } => {
            f(dest);
            f(value);
            f(size);
        }
        ExpressionKind::TableCopy {
            dest, src, size, ..
        } => {
            f(dest);
            f(src);
            f(size);
        }
        ExpressionKind::TableInit {
            dest, offset, size, ..
        } => {
            f(dest);
            f(offset);
            f(size);
        }
        ExpressionKind::StructNew { operands, .. } => {
            for operand in operands.iter() {
                f(operand);
            }
        }
        ExpressionKind::StructGet { ptr, .. } => f(ptr),
        ExpressionKind::StructSet { ptr, value, .. } => {
            f(ptr);
            f(value);
        }
        ExpressionKind::ArrayNew { size, init, .. } => {
            f(size);
            if let Some(i) = init {
                f(i);
            }
        }
        ExpressionKind::ArrayGet { ptr, index, .. } => {
            f(ptr);
            f(index);
        }
        ExpressionKind::ArraySet {
            ptr, index, value, ..
        } => {
            f(ptr);
            f(index);
            f(value);
        }
        ExpressionKind::ArrayLen { ptr } => f(ptr),
        ExpressionKind::Try {
            body, catch_bodies, ..
        } => {
            f(body);
            for cb in catch_bodies.iter() {
                f(cb);
            }
        }
        ExpressionKind::Throw { operands, .. } => {
            for op in operands.iter() {
                f(op);
            }
        }
        ExpressionKind::I31New { value } => {
            f(value);
        }
        ExpressionKind::I31Get { i31, .. } => {
            f(i31);
        }
        ExpressionKind::Nop
        | ExpressionKind::Unreachable
        | ExpressionKind::Const(_)
        | ExpressionKind::LocalGet { .. }
        | ExpressionKind::GlobalGet { .. }
        | ExpressionKind::MemorySize
        | ExpressionKind::TableSize { .. }
        | ExpressionKind::AtomicFence
        | ExpressionKind::RefNull { .. }
        | ExpressionKind::RefFunc { .. }
        | ExpressionKind::DataDrop { .. }
        | ExpressionKind::ElemDrop { .. }
        | ExpressionKind::Pop { .. }
        | ExpressionKind::Rethrow { .. }
        | ExpressionKind::RefTest { .. }
        | ExpressionKind::RefCast { .. }
        | ExpressionKind::BrOn { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::IrBuilder;
    use crate::ir::{Literal, Type};
    use bumpalo::collections::Vec as BumpVec;
    use bumpalo::Bump;

    #[test]
    fn test_simplify_locals_basic() {
        // Test that SimplifyLocals can be created and run
        let mut pass = SimplifyLocals::new();
        assert_eq!(pass.name(), "SimplifyLocals");

        // Create empty module
        let bump = Bump::new();
        let mut module = Module::new(&bump);
        pass.run(&mut module);
    }

    #[test]
    fn test_simplify_locals_with_options() {
        // Test different option combinations
        let pass1 = SimplifyLocals::with_options(true, true, true);
        assert!(pass1.allow_tee && pass1.allow_structure && pass1.allow_nesting);

        let pass2 = SimplifyLocals::flat();
        assert!(!pass2.allow_tee && !pass2.allow_structure && !pass2.allow_nesting);
    }

    #[test]
    fn test_function_context() {
        // Test FunctionContext creation
        let ctx = FunctionContext::new(true, true, true);
        assert!(ctx.allow_tee);
        assert!(ctx._allow_structure);
        assert!(ctx._allow_nesting);
        assert!(ctx.first_cycle);
    }

    #[test]
    fn test_sink_to_tee() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let mut module = Module::new(&bump);

        // (block
        //   (local.set 0 (const 42))
        //   (drop (local.get 0))
        //   (drop (local.get 0))
        // )
        let mut list = BumpVec::new_in(&bump);
        list.push(builder.local_set(0, builder.const_(Literal::I32(42))));
        list.push(builder.drop(builder.local_get(0, Type::I32)));
        list.push(builder.drop(builder.local_get(0, Type::I32)));

        let block = builder.block(None, list, Type::NONE);

        module.functions.push(crate::ir::module::Function {
            name: "test".to_string(),
            type_idx: None,
            params: Type::NONE,
            results: Type::NONE,
            vars: vec![Type::I32],
            body: Some(block),
            local_names: vec![String::new(); 1],
        });

        let mut pass = SimplifyLocals::new();
        pass.run(&mut module);

        let body = module.functions[0]
            .body
            .expect("Function should have a body");
        if let ExpressionKind::Block { list, .. } = &body.kind {
            assert_eq!(list.len(), 3);
            // After all cycles:
            // 1. (nop) - the original set was sunk
            assert!(matches!(list[0].kind, ExpressionKind::Nop));
            // 2. (nop) - the sunk set (from tee) was sunk again
            assert!(matches!(list[1].kind, ExpressionKind::Nop));
            // 3. (drop (const 42)) - final destination
            if let ExpressionKind::Drop { value } = &list[2].kind {
                assert!(matches!(
                    value.kind,
                    ExpressionKind::Const(Literal::I32(42))
                ));
            } else {
                panic!("Expected Drop(Const), got {:?}", list[2].kind);
            }
        } else {
            panic!("Expected Block, got {:?}", body.kind);
        }
    }

    #[test]
    fn test_structure_opt() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let mut module = Module::new(&bump);

        // (if (local.get 1) (local.set 0 (const 1)) (local.set 0 (const 2)))
        // -> (local.set 0 (if (local.get 1) (const 1) (const 2)))

        let condition = builder.local_get(1, Type::I32);
        let mut true_list = BumpVec::new_in(&bump);
        true_list.push(builder.local_set(0, builder.const_(Literal::I32(1))));
        let if_true = builder.block(None, true_list, Type::NONE);

        let mut false_list = BumpVec::new_in(&bump);
        false_list.push(builder.local_set(0, builder.const_(Literal::I32(2))));
        let if_false = Some(builder.block(None, false_list, Type::NONE));

        let if_expr = builder.if_(condition, if_true, if_false, Type::NONE);

        module.functions.push(crate::ir::module::Function {
            name: "test".to_string(),
            type_idx: None,
            params: Type::NONE,
            results: Type::NONE,
            vars: vec![Type::I32, Type::I32],
            body: Some(if_expr),
            local_names: vec![String::new(); 2],
        });

        let mut pass = SimplifyLocals::new();
        pass.run(&mut module);

        let body = module.functions[0]
            .body
            .expect("Function should have a body");
        if let ExpressionKind::LocalSet { index, value } = &body.kind {
            assert_eq!(*index, 0);
            assert!(matches!(value.kind, ExpressionKind::If { .. }));
        } else {
            panic!("Expected LocalSet, got {:?}", body.kind);
        }
    }

    #[test]
    fn test_block_return_opt() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let mut module = Module::new(&bump);

        let mut list = BumpVec::new_in(&bump);
        list.push(builder.local_set(0, builder.const_(Literal::I32(1))));
        let block = builder.block(Some("label"), list, Type::NONE);

        module.functions.push(crate::ir::module::Function {
            name: "test".to_string(),
            params: Type::NONE,
            results: Type::NONE,
            vars: vec![Type::I32],
            body: Some(block),
            local_names: vec![String::new(); 1],
            type_idx: None,
        });

        let mut pass = SimplifyLocals::new();
        pass.run(&mut module);

        let body = module.functions[0].body.unwrap();
        if let ExpressionKind::LocalSet { index, value: _ } = &body.kind {
            assert_eq!(*index, 0);
        } else {
            panic!("Expected LocalSet, got {:?}", body.kind);
        }
    }

    #[test]
    fn test_if_return_opt() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let mut module = Module::new(&bump);

        let condition = builder.local_get(1, Type::I32);
        let mut true_list = BumpVec::new_in(&bump);
        true_list.push(builder.local_set(0, builder.const_(Literal::I32(1))));
        let if_true = builder.block(None, true_list, Type::NONE);

        let if_expr = builder.if_(condition, if_true, None, Type::NONE);

        module.functions.push(crate::ir::module::Function {
            name: "test".to_string(),
            params: Type::NONE,
            results: Type::NONE,
            vars: vec![Type::I32, Type::I32],
            body: Some(if_expr),
            local_names: vec![String::new(); 2],
            type_idx: None,
        });

        let mut pass = SimplifyLocals::new();
        pass.run(&mut module);

        let body = module.functions[0].body.unwrap();
        if let ExpressionKind::LocalSet { index, value } = &body.kind {
            assert_eq!(*index, 0);
            if let ExpressionKind::If { if_false, .. } = &value.kind {
                assert!(if_false.is_some());
            } else {
                panic!("Expected If in LocalSet");
            }
        } else {
            panic!("Expected LocalSet, got {:?}", body.kind);
        }
    }

    #[test]
    fn test_loop_return_opt() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let mut module = Module::new(&bump);

        let mut list = BumpVec::new_in(&bump);
        list.push(builder.local_set(0, builder.const_(Literal::I32(1))));
        let block = builder.block(None, list, Type::NONE);
        let loop_expr = builder.loop_(Some("label"), block, Type::NONE);

        module.functions.push(crate::ir::module::Function {
            name: "test".to_string(),
            params: Type::NONE,
            results: Type::NONE,
            vars: vec![Type::I32],
            body: Some(loop_expr),
            local_names: vec![String::new(); 1],
            type_idx: None,
        });

        let mut pass = SimplifyLocals::new();
        pass.run(&mut module);

        let body = module.functions[0].body.unwrap();
        if let ExpressionKind::LocalSet { index, value } = &body.kind {
            assert_eq!(*index, 0);
            assert!(matches!(value.kind, ExpressionKind::Loop { .. }));
        } else {
            panic!("Expected LocalSet, got {:?}", body.kind);
        }
    }
}
