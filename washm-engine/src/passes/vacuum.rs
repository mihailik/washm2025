use crate::analysis::effects::EffectAnalyzer;
use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::module::Module;
use crate::ir::{Literal, Type};
use crate::passes::Pass;

/// Removes redundant code such as nops, unused expressions, and simplifies blocks/ifs.
pub struct Vacuum;

impl Pass for Vacuum {
    fn name(&self) -> &str {
        "Vacuum"
    }

    fn run(&mut self, module: &mut Module) {
        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        let mut optimizer = VacuumOptimizer {
            arena,
            root: None,
            next_targets: Vec::new(),
        };
        for (_, func) in functions.iter_mut() {
            let result_used = func.results != Type::NONE;
            if let Some(body) = func.body_mut() {
                optimizer.root = Some(*body);
                optimizer.next_targets.clear();
                optimizer.process(body);
                // Final function-level optimization (like Binaryen's visitFunction)
                optimizer.finalize_function(body, result_used);
                optimizer.root = None;
            }
        }
    }
}

// removed HashSet import

struct VacuumOptimizer<'a> {
    arena: &'a mut IdArena<ExpressionMarker, Expression>,
    // Root expression id for the current function being processed.
    root: Option<ExpressionId>,
    // Stack of expressions that are reachable via fallthrough from the current point.
    // A break to any of these is redundant.
    next_targets: Vec<ExpressionId>,
}

impl<'a> VacuumOptimizer<'a> {
    /// Try to optimize an expression when its result is unused. Returns:
    /// - None if the expression can be removed entirely
    /// - Some(id) if the expression should be replaced with `id` (may be same id)
    fn optimize(
        &mut self,
        mut expr_id: ExpressionId,
        result_used: bool,
        mut type_matters: bool,
    ) -> Option<ExpressionId> {
        let original_type = self.arena[expr_id].type_;
        if original_type == Type::UNREACHABLE {
            return Some(expr_id);
        }
        if original_type == Type::NONE {
            type_matters = true;
        }
        if result_used {
            return Some(expr_id);
        }

        loop {
            // If a replacement changes the type, and the type matters, return the
            // previous one and stop.
            if type_matters && self.arena[expr_id].type_ != original_type {
                return Some(expr_id);
            }

            // Do not try to optimize these container nodes here.
            match &self.arena[expr_id].kind {
                ExpressionKind::Drop { .. }
                | ExpressionKind::Block { .. }
                | ExpressionKind::If { .. }
                | ExpressionKind::Loop { .. }
                | ExpressionKind::Try { .. } => return Some(expr_id),
                ExpressionKind::LocalSet { id, value } | ExpressionKind::LocalTee { id, value } => {
                    if let ExpressionKind::LocalGet { id: get_id } = &self.arena[*value].kind {
                        if id == get_id {
                            if result_used {
                                return Some(*value);
                            } else {
                                return None;
                            }
                        }
                    }
                }
                _ => {}
            }

            // If the node itself has unremovable side-effects (ignoring child
            // effects), stop. We approximate this by checking the operator kind
            // for intrinsic side effects.
            if matches!(
                self.arena[expr_id].kind,
                ExpressionKind::LocalSet { .. }
                    | ExpressionKind::GlobalSet { .. }
                    | ExpressionKind::Store { .. }
                    | ExpressionKind::Call { .. }
                    | ExpressionKind::CallIndirect { .. }
                    | ExpressionKind::Return { .. }
                    | ExpressionKind::Break { .. }
                    | ExpressionKind::Switch { .. }
                    | ExpressionKind::Unreachable
                    | ExpressionKind::MemoryGrow { .. }
                    | ExpressionKind::Throw { .. }
                    | ExpressionKind::TableSet { .. }
                    | ExpressionKind::TableGrow { .. }
                    | ExpressionKind::AtomicRMW { .. }
                    | ExpressionKind::AtomicCmpxchg { .. }
                    | ExpressionKind::AtomicWait { .. }
                    | ExpressionKind::AtomicNotify { .. }
                    | ExpressionKind::TableInit { .. }
                    | ExpressionKind::MemoryInit { .. }
                    | ExpressionKind::MemoryCopy { .. }
                    | ExpressionKind::MemoryFill { .. }
            ) {
                return Some(expr_id);
            }

            // Collect children that have side-effects (we must preserve these)
            let mut children_with_effects: Vec<ExpressionId> = Vec::new();
            self.arena[expr_id].kind.for_each_child(|child| {
                if EffectAnalyzer::analyze(child, self.arena).has_side_effects() {
                    children_with_effects.push(child);
                }
            });

            if children_with_effects.is_empty() {
                // nothing remains
                if self.contains_memory_copy(expr_id) {
                    eprintln!(
                        "VACUUM-DIAG: removing subtree that contains MemoryCopy; node kind={:?}",
                        self.arena[expr_id].kind
                    );
                }
                return None;
            }

            if children_with_effects.len() == 1 {
                // we can replace this node with that single effectful child
                expr_id = children_with_effects[0];
                continue;
            }

            // Multiple side-effectful children: if we can construct a dummy value
            // of the proper type, replace the node with a block of those children
            // followed by the dummy.
            // Heuristic: avoid replacing an expression with children+dummy when
            // the effectful children are local stores/tees etc., since that is a
            // common pattern for evaluating operands (it would change the
            // computed value). If any child is a local/global set or store, give
            // up and keep the original expression.
            if children_with_effects.iter().any(|&c| {
                matches!(
                    self.arena[c].kind,
                    ExpressionKind::LocalSet { .. }
                        | ExpressionKind::LocalTee { .. }
                        | ExpressionKind::GlobalSet { .. }
                        | ExpressionKind::Store { .. }
                )
            }) {
                return Some(expr_id);
            }

            if let Some(dummy) = self.make_default_value(original_type) {
                let mut list = children_with_effects.clone();
                list.push(dummy);
                let block_id = Expression::alloc(
                    self.arena,
                    ExpressionKind::Block { name: None, list },
                    original_type,
                );
                return Some(block_id);
            }

            // Otherwise, give up.
            return Some(expr_id);
        }
    }

    /// Return Some(expr_id) if we can create a default/dummy value for `ty`.
    fn make_default_value(&mut self, ty: Type) -> Option<ExpressionId> {
        if ty == Type::NONE || ty == Type::UNREACHABLE {
            return None;
        }
        if ty.is_concrete() {
            let lit = match ty {
                Type::I32 => Literal::I32(0),
                Type::I64 => Literal::I64(0),
                Type::F32 => Literal::F32(0.0),
                Type::F64 => Literal::F64(0.0),
                Type::V128 => Literal::V128([0u8; 16]),
                _ => return None,
            };
            let id = self
                .arena
                .alloc(Expression::new(ExpressionKind::Const(lit), ty));
            return Some(id);
        }
        // Refs: use RefNull
        if ty.is_ref() {
            let id = self
                .arena
                .alloc(Expression::new(ExpressionKind::RefNull { type_: ty }, ty));
            return Some(id);
        }
        None
    }

    fn get_single_break(&self, expr_id: ExpressionId) -> Option<ExpressionId> {
        match &self.arena[expr_id].kind {
            ExpressionKind::Break {
                target,
                condition: None,
                value: None,
            } => Some(*target),
            ExpressionKind::Block { name: None, list } if list.len() == 1 => {
                self.get_single_break(list[0])
            }
            _ => None,
        }
    }

    fn process(&mut self, expr_id: &mut ExpressionId) {
        // First, attempt to apply function-level optimizations like in Binaryen's
        // `optimize` for nodes where results are unused. We still recurse as
        // before, but use `optimize` at strategic points.
        let _block_type = self.arena[*expr_id].type_;
        let expr_clone = self.arena[*expr_id].clone();
        match expr_clone.kind {
            ExpressionKind::Block { name, mut list } => {
                // Pre-filter list to remove Nops and find trailing unreachable code
                let mut filtered_list = Vec::with_capacity(list.len());
                for &item in &list {
                    if !matches!(self.arena[item].kind, ExpressionKind::Nop) {
                        filtered_list.push(item);
                        if self.arena[item].type_ == Type::UNREACHABLE {
                            break;
                        }
                    }
                }
                list = filtered_list;

                // Recurse first and update list in place
                let list_len = list.len();
                let mut processed_list = Vec::with_capacity(list_len);

                for i in 0..list_len {
                    // An instruction is at the "end" of this block if nothing follows it
                    // effectively (only Nops or nothing at all).
                    let is_last = i == list_len - 1;
                    let mut current_targets = if is_last {
                        self.next_targets.clone()
                    } else {
                        Vec::new()
                    };

                    if is_last && name.is_some() {
                        current_targets.push(*expr_id);
                    }

                    let saved_targets = std::mem::replace(&mut self.next_targets, current_targets);
                    let mut item = list[i];
                    self.process(&mut item);
                    processed_list.push(item);
                    self.next_targets = saved_targets;

                    if self.arena[item].type_ == Type::UNREACHABLE {
                        break;
                    }
                }
                list = processed_list;

                let mut new_list = Vec::new();
                let mut i = 0;
                let block_type = self.arena[*expr_id].type_;
                while i < list.len() {
                    let child_id = list[i];

                    // Remove degenerate pattern: a br_if that targets this block and whose
                    // condition is constant true with no value. It's equivalent to a no-op
                    // since both branches continue after the block.
                    if let ExpressionKind::Break {
                        target,
                        condition: Some(cond),
                        value: None,
                    } = &self.arena[child_id].kind
                    {
                        if *target == *expr_id {
                            if let ExpressionKind::Const(lit) = &self.arena[*cond].kind {
                                let is_true = match lit {
                                    Literal::I32(v) => *v != 0,
                                    Literal::I64(v) => *v != 0,
                                    _ => false,
                                };
                                if is_true {
                                    i += 1;
                                    continue;
                                }
                            }
                        }
                    }

                    // Attempt to optimize child when it's not the last (or when the
                    // block result isn't used), mirroring Binaryen's logic.
                    let is_last = i == list.len() - 1;
                    let used = is_last && block_type != Type::NONE;
                    if let Some(opt_id) = self.optimize(child_id, used, true) {
                        // If this optimization replaced the child with a block that
                        // is itself a list, flatten anonymous blocks in place.
                        if let ExpressionKind::Block {
                            name: None,
                            list: child_list,
                        } = self.arena[opt_id].kind.clone()
                        {
                            for cc in child_list {
                                self.add_to_block(&mut new_list, cc, block_type, is_last);
                            }
                        } else {
                            self.add_to_block(&mut new_list, opt_id, block_type, is_last);
                        }
                    } else {
                        // optimized away entirely
                    }

                    if self.arena[child_id].type_ == Type::UNREACHABLE {
                        break;
                    }
                    i += 1;
                }

                // Find if any Break targets this specific block. If not, we can remove its name.
                let mut is_targeted = false;
                if name.is_some() {
                    if let Some(root) = self.root {
                        let mut stack = vec![root];
                        while let Some(eid) = stack.pop() {
                            if let ExpressionKind::Break { target, .. } = &self.arena[eid].kind {
                                if *target == *expr_id {
                                    is_targeted = true;
                                    break;
                                }
                            }
                            self.arena[eid]
                                .kind
                                .for_each_child(|child| stack.push(child));
                        }
                    }
                }

                let effective_name = if is_targeted { name } else { None };

                // If a block is empty and not targeted, removing it is always safe.
                if new_list.is_empty() {
                    if !is_targeted {
                        self.arena[*expr_id].kind = ExpressionKind::Nop;
                        self.arena[*expr_id].type_ = Type::NONE;
                        return;
                    }
                }

                // If a block has no name and only one child, flatten it.
                if effective_name.is_none() {
                    if new_list.is_empty() {
                        self.arena[*expr_id].kind = ExpressionKind::Nop;
                        self.arena[*expr_id].type_ = Type::NONE;
                        return;
                    } else if new_list.len() == 1 {
                        let child_id = new_list[0];
                        *expr_id = child_id;
                        // Since we just replaced the block with its child, we should
                        // potentially re-process to ensure any nested trivialities are caught.
                        self.process(expr_id);
                        return;
                    }
                }

                self.arena[*expr_id].kind = ExpressionKind::Block {
                    name: effective_name,
                    list: new_list,
                };
            }
            ExpressionKind::If {
                name,
                mut condition,
                mut if_true,
                mut if_false,
            } => {
                self.process(&mut condition);

                let mut arm_targets = self.next_targets.clone();
                if name.is_some() {
                    arm_targets.push(*expr_id);
                }

                let saved_targets = std::mem::replace(&mut self.next_targets, arm_targets);
                self.process(&mut if_true);
                if let Some(mut f) = if_false {
                    self.process(&mut f);
                    if_false = Some(f);
                }
                self.next_targets = saved_targets;

                // Arm merging: if both arms are effectively the same side-effect, merging them
                // can simplify the IR. For now, detect identical Breaks.
                if let Some(f) = if_false {
                    if let (Some(t1), Some(t2)) =
                        (self.get_single_break(if_true), self.get_single_break(f))
                    {
                        if t1 == t2 {
                            // if (cond) break; else break;  =>  drop(cond); break;
                            let drop_id = self.arena.alloc(Expression::new(
                                ExpressionKind::Drop { value: condition },
                                Type::NONE,
                            ));
                            let br_id =
                                if matches!(self.arena[if_true].kind, ExpressionKind::Break { .. })
                                {
                                    if_true
                                } else {
                                    f
                                };
                            self.arena[*expr_id].kind = ExpressionKind::Block {
                                name: None,
                                list: vec![drop_id, br_id],
                            };
                            self.arena[*expr_id].type_ = self.arena[br_id].type_;
                            self.process(expr_id);
                            return;
                        }
                    }
                }

                // constant folding (preserve any side-effects in the condition)
                if let ExpressionKind::Const(lit) = &self.arena[condition].kind {
                    let is_true = match lit {
                        Literal::I32(v) => *v != 0,
                        Literal::I64(v) => *v != 0,
                        _ => false,
                    };
                    let cond_has_effects =
                        EffectAnalyzer::analyze(condition, self.arena).has_side_effects();
                    if is_true {
                        if cond_has_effects {
                            let drop_id = self.arena.alloc(Expression::new(
                                ExpressionKind::Drop { value: condition },
                                Type::NONE,
                            ));
                            self.arena[*expr_id].kind = ExpressionKind::Block {
                                name: None,
                                list: vec![drop_id, if_true],
                            };
                            self.arena[*expr_id].type_ = self.arena[if_true].type_;
                            self.process(expr_id);
                            return;
                        }
                        *expr_id = if_true;
                        return;
                    } else if let Some(false_branch) = if_false {
                        if cond_has_effects {
                            let drop_id = self.arena.alloc(Expression::new(
                                ExpressionKind::Drop { value: condition },
                                Type::NONE,
                            ));
                            self.arena[*expr_id].kind = ExpressionKind::Block {
                                name: None,
                                list: vec![drop_id, false_branch],
                            };
                            self.arena[*expr_id].type_ = self.arena[false_branch].type_;
                            self.process(expr_id);
                            return;
                        }
                        *expr_id = false_branch;
                        return;
                    } else {
                        if cond_has_effects {
                            self.arena[*expr_id].kind = ExpressionKind::Drop { value: condition };
                            self.arena[*expr_id].type_ = Type::NONE;
                            return;
                        }
                        self.arena[*expr_id].kind = ExpressionKind::Nop;
                        self.arena[*expr_id].type_ = Type::NONE;
                        return;
                    }
                }

                if let Some(f) = if_false {
                    if matches!(self.arena[f].kind, ExpressionKind::Nop) {
                        if_false = None;
                    }
                }

                if matches!(self.arena[if_true].kind, ExpressionKind::Nop) {
                    if let Some(f) = if_false.take() {
                        if_true = f;
                        // Invert condition
                        condition = self.arena.alloc(Expression::new(
                            ExpressionKind::Unary {
                                op: crate::ir::ops::UnaryOp::EqZInt32,
                                value: condition,
                            },
                            Type::I32,
                        ));
                    }
                }

                if matches!(self.arena[if_true].kind, ExpressionKind::Nop) && if_false.is_none() {
                    self.arena[*expr_id].kind = ExpressionKind::Drop { value: condition };
                    self.arena[*expr_id].type_ = Type::NONE;
                    self.process(expr_id);
                    return;
                }

                self.arena[*expr_id].kind = ExpressionKind::If {
                    name,
                    condition,
                    if_true,
                    if_false,
                };
            }
            ExpressionKind::Break {
                target,
                mut condition,
                mut value,
            } => {
                if value.is_none() && self.next_targets.contains(&target) {
                    if let Some(mut cond) = condition {
                        self.process(&mut cond);
                        self.arena[*expr_id].kind = ExpressionKind::Drop { value: cond };
                        self.arena[*expr_id].type_ = Type::NONE;
                    } else {
                        self.arena[*expr_id].kind = ExpressionKind::Nop;
                        self.arena[*expr_id].type_ = Type::NONE;
                    }
                } else {
                    if let Some(mut c) = condition {
                        self.process(&mut c);
                        condition = Some(c);
                    }
                    if let Some(mut v) = value {
                        self.process(&mut v);
                        value = Some(v);
                    }
                    self.arena[*expr_id].kind = ExpressionKind::Break {
                        target,
                        condition,
                        value,
                    };
                }
            }
            ExpressionKind::Drop { mut value } => {
                // Recurse into the value we are dropping
                self.process(&mut value);

                // If the value is a LocalTee, convert Drop(LocalTee) -> LocalSet
                if let ExpressionKind::LocalTee { id, value: tee_val } =
                    self.arena[value].kind.clone()
                {
                    self.arena[*expr_id].kind = ExpressionKind::LocalSet { id, value: tee_val };
                    self.arena[*expr_id].type_ = Type::NONE;
                    return;
                }

                // Sink drops into if arms when the other arm is unreachable
                if let ExpressionKind::If {
                    name,
                    condition,
                    if_true,
                    if_false,
                } = self.arena[value].kind.clone()
                {
                    if let Some(false_id) = if_false {
                        if self.arena[if_true].type_ == Type::UNREACHABLE
                            && self.arena[false_id].type_.is_concrete()
                        {
                            // make a new drop for the false arm and replace the if's false with it
                            let new_drop = self.arena.alloc(Expression::new(
                                ExpressionKind::Drop { value: false_id },
                                Type::NONE,
                            ));
                            self.arena[value].kind = ExpressionKind::If {
                                name: name.clone(),
                                condition,
                                if_true,
                                if_false: Some(new_drop),
                            };
                            self.arena[value].type_ = Type::NONE;
                            *expr_id = value;
                            return;
                        } else if self.arena[false_id].type_ == Type::UNREACHABLE
                            && self.arena[if_true].type_.is_concrete()
                        {
                            let new_drop = self.arena.alloc(Expression::new(
                                ExpressionKind::Drop { value: if_true },
                                Type::NONE,
                            ));
                            self.arena[value].kind = ExpressionKind::If {
                                name: name.clone(),
                                condition,
                                if_true: new_drop,
                                if_false: None,
                            };
                            self.arena[value].type_ = Type::NONE;
                            *expr_id = value;
                            return;
                        }
                    }
                }

                if !EffectAnalyzer::analyze(value, self.arena).has_side_effects() {
                    self.arena[*expr_id].kind = ExpressionKind::Nop;
                    self.arena[*expr_id].type_ = Type::NONE;
                } else {
                    self.arena[*expr_id].kind = ExpressionKind::Drop { value };
                }
            }
            ExpressionKind::Loop { name, mut body } => {
                let mut loop_targets = self.next_targets.clone();
                // br to a loop is a restart/continue. It is "trivial" if it's at
                // the very end of the loop body.
                loop_targets.push(*expr_id);

                let saved_targets = std::mem::replace(&mut self.next_targets, loop_targets);
                self.process(&mut body);
                self.next_targets = saved_targets;

                self.arena[*expr_id].kind = ExpressionKind::Loop { name, body };
            }
            ExpressionKind::LocalSet { id, mut value } => {
                self.process(&mut value);
                // Case 1: v = v
                if let ExpressionKind::LocalGet { id: get_id } = &self.arena[value].kind {
                    if id == *get_id {
                        self.arena[*expr_id].kind = ExpressionKind::Nop;
                        self.arena[*expr_id].type_ = Type::NONE;
                        return;
                    }
                }
                // Case 2: v = (v := val)
                if let ExpressionKind::LocalTee {
                    id: tee_id,
                    value: _,
                } = &self.arena[value].kind
                {
                    if id == *tee_id {
                        *expr_id = value;
                        return;
                    }
                }
                self.arena[*expr_id].kind = ExpressionKind::LocalSet { id, value };
            }
            ExpressionKind::LocalTee { id, mut value } => {
                self.process(&mut value);
                // Case 1: v := v
                if let ExpressionKind::LocalGet { id: get_id } = &self.arena[value].kind {
                    if id == *get_id {
                        *expr_id = value;
                        return;
                    }
                }
                // Case 2: v := (v := val)
                if let ExpressionKind::LocalTee {
                    id: tee_id,
                    value: _,
                } = &self.arena[value].kind
                {
                    if id == *tee_id {
                        *expr_id = value;
                        return;
                    }
                }
                self.arena[*expr_id].kind = ExpressionKind::LocalTee { id, value };
            }
            ExpressionKind::Try {
                name,
                body,
                catch_tags,
                catch_bodies,
                delegate,
            } => {
                // If the try's body does not throw, we can replace the Try with its body.
                if !EffectAnalyzer::analyze(body, self.arena)
                    .contains(crate::analysis::effects::Effect::THROWS)
                {
                    *expr_id = body;
                    return;
                }

                // Otherwise, keep visiting children as needed.
                // Visit body and catches
                let saved_targets = self.next_targets.clone();
                self.next_targets.push(*expr_id);

                let mut b = body;
                self.process(&mut b);
                let mut new_catches: Vec<ExpressionId> = Vec::new();
                for c in catch_bodies.iter() {
                    let mut cc = *c;
                    self.process(&mut cc);
                    new_catches.push(cc);
                }

                self.next_targets = saved_targets;

                self.arena[*expr_id].kind = ExpressionKind::Try {
                    name,
                    body: b,
                    catch_tags,
                    catch_bodies: new_catches,
                    delegate,
                };
            }
            _ => self.visit_children(expr_id),
        }
    }

    fn add_to_block(
        &self,
        list: &mut Vec<ExpressionId>,
        child_id: ExpressionId,
        block_type: Type,
        is_last: bool,
    ) {
        if matches!(self.arena[child_id].kind, ExpressionKind::Nop) {
            if is_last && block_type != Type::NONE && block_type != Type::UNREACHABLE {
                list.push(child_id);
            }
            return;
        }
        if !is_last
            && self.arena[child_id].type_ != Type::UNREACHABLE
            && !EffectAnalyzer::analyze(child_id, self.arena).has_side_effects()
        {
            return;
        }
        list.push(child_id);
    }

    fn visit_children(&mut self, expr_id: &mut ExpressionId) {
        let mut expr_kind = self.arena[*expr_id].kind.clone();
        match &mut expr_kind {
            ExpressionKind::Unary { ref mut value, .. }
            | ExpressionKind::LocalSet { ref mut value, .. }
            | ExpressionKind::LocalTee { ref mut value, .. }
            | ExpressionKind::GlobalSet { ref mut value, .. }
            | ExpressionKind::Load {
                ptr: ref mut value, ..
            }
            | ExpressionKind::MemoryGrow {
                delta: ref mut value,
                ..
            } => {
                self.process(value);
            }
            ExpressionKind::Binary {
                ref mut left,
                ref mut right,
                ..
            }
            | ExpressionKind::Store {
                ptr: ref mut left,
                value: ref mut right,
                ..
            } => {
                self.process(left);
                self.process(right);
            }
            ExpressionKind::Call { operands, .. }
            | ExpressionKind::CallIndirect { operands, .. } => {
                for op in operands.iter_mut() {
                    self.process(op);
                }
            }
            ExpressionKind::Select {
                ref mut if_true,
                ref mut if_false,
                ref mut condition,
            } => {
                self.process(if_true);
                self.process(if_false);
                self.process(condition);
            }
            ExpressionKind::Switch {
                ref mut condition, ..
            }
            | ExpressionKind::Return {
                value: Some(ref mut condition),
            } => {
                self.process(condition);
            }
            ExpressionKind::MemoryCopy {
                ref mut dest,
                ref mut src,
                ref mut size,
                ..
            } => {
                self.process(dest);
                self.process(src);
                self.process(size);
            }
            ExpressionKind::MemoryFill {
                ref mut dest,
                ref mut value,
                ref mut size,
                ..
            } => {
                self.process(dest);
                self.process(value);
                self.process(size);
            }
            ExpressionKind::MemoryInit {
                ref mut dest,
                ref mut offset,
                ref mut size,
                ..
            } => {
                self.process(dest);
                self.process(offset);
                self.process(size);
            }
            ExpressionKind::DataDrop { .. } | ExpressionKind::ElemDrop { .. } => {}
            _ => {}
        }
        self.arena[*expr_id].kind = expr_kind;
    }

    /// Does the subtree rooted at `expr_id` contain a MemoryCopy node?
    fn contains_memory_copy(&self, expr_id: ExpressionId) -> bool {
        if matches!(self.arena[expr_id].kind, ExpressionKind::MemoryCopy { .. }) {
            return true;
        }
        let mut found = false;
        self.arena[expr_id].kind.for_each_child(|child| {
            if !found && self.contains_memory_copy(child) {
                found = true;
            }
        });
        found
    }

    /// Apply final function-level optimization: try to optimize the function body
    /// as a whole using `optimize` semantics (similar to Binaryen's visitFunction).
    fn finalize_function(&mut self, expr_id: &mut ExpressionId, result_used: bool) {
        if let Some(opt) = self.optimize(*expr_id, result_used, true) {
            *expr_id = opt;
        } else {
            // Sanity check: do not elide a function body that has side-effects.
            if EffectAnalyzer::analyze(*expr_id, self.arena).has_side_effects() {
                panic!(
                    "finalize_function would elide expression with side-effects: {:?}",
                    self.arena[*expr_id].kind
                );
            }
            self.arena[*expr_id].kind = ExpressionKind::Nop;
            self.arena[*expr_id].type_ = Type::NONE;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::{Expression, ExpressionKind};
    use crate::ir::module::Function;
    use crate::ir::{Literal, Type};

    #[test]
    fn test_vacuum_block_flatten() {
        let mut module = Module::new();
        let inner_const = module.expressions.alloc(Expression::new(
            ExpressionKind::Const(Literal::I32(42)),
            Type::I32,
        ));
        let inner_block = module.expressions.alloc(Expression {
            kind: ExpressionKind::Block {
                name: None,
                list: vec![inner_const],
            },
            type_: Type::I32,
        });

        let outer_block = module.expressions.alloc(Expression {
            kind: ExpressionKind::Block {
                name: None,
                list: vec![inner_block],
            },
            type_: Type::I32,
        });

        let func_id = module.add_function(Function::new(
            "t".into(),
            Type::NONE,
            Type::I32,
            vec![],
            outer_block,
        ));

        let mut pass = Vacuum;
        pass.run(&mut module);

        let body_id = module.functions[func_id].body().unwrap();
        assert!(matches!(
            module.expressions[body_id].kind,
            ExpressionKind::Const(_)
        ));
    }

    #[test]
    fn test_vacuum_remove_trivial_br_if() {
        let mut module = Module::new();

        // const true
        let const_true = module.expressions.alloc(Expression::new(
            ExpressionKind::Const(Literal::I32(1)),
            Type::I32,
        ));

        // Placeholder for inner_block
        let inner_block = module
            .expressions
            .alloc(Expression::new(ExpressionKind::Nop, Type::NONE));

        // break to target "inner_block" with condition const_true
        let break_expr = module.expressions.alloc(Expression::new(
            ExpressionKind::Break {
                target: inner_block,
                condition: Some(const_true),
                value: None,
            },
            Type::NONE,
        ));

        // named inner block with the break as its only child
        module.expressions[inner_block] = Expression::new(
            ExpressionKind::Block {
                name: Some("lbl".to_string()),
                list: vec![break_expr],
            },
            Type::NONE,
        );

        // following dummy const to ensure it's preserved after simplification
        let following = module.expressions.alloc(Expression::new(
            ExpressionKind::Const(Literal::I32(7)),
            Type::I32,
        ));

        let outer_block = module.expressions.alloc(Expression::new(
            ExpressionKind::Block {
                name: None,
                list: vec![inner_block, following],
            },
            Type::I32,
        ));

        let func_id = module.add_function(Function::new(
            "t".into(),
            Type::NONE,
            Type::I32,
            vec![],
            outer_block,
        ));

        let mut pass = Vacuum;
        pass.run(&mut module);

        let body_id = module.functions[func_id].body().unwrap();
        // After vacuum, the inner trivial block should be removed and the following const should be present.
        match &module.expressions[body_id].kind {
            ExpressionKind::Block { list, .. } => {
                assert!(matches!(
                    module.expressions[list[0]].kind,
                    ExpressionKind::Const(_)
                ));
            }
            ExpressionKind::Const(_) => { /* also acceptable: outer block collapsed to single const */
            }
            _ => panic!(
                "unexpected body kind: {:?}",
                module.expressions[body_id].kind
            ),
        }
    }

    #[test]
    fn test_vacuum_optimize_multiple_children_dummy() {
        // a wrapper node with multiple side-effectful children should be replaced by the children + a dummy to match the type
        let mut module = Module::new();
        // start with a dummy body
        let dummy = module.expressions.alloc(Expression::new(
            ExpressionKind::Const(Literal::I32(0)),
            Type::I32,
        ));
        let func_id = module.add_function(Function::new(
            "t".into(),
            Type::NONE,
            Type::NONE,
            vec![Type::I32, Type::I32],
            dummy,
        ));
        // Instead, use two calls as side-effectful children (not local-sets),
        // since those are a safer pattern to transform.
        let f1 = module.add_function(Function::new(
            "a".into(),
            Type::NONE,
            Type::I32,
            vec![],
            dummy,
        ));
        let f2 = module.add_function(Function::new(
            "b".into(),
            Type::NONE,
            Type::I32,
            vec![],
            dummy,
        ));
        let call1 = module.expressions.alloc(Expression::new(
            ExpressionKind::Call {
                target: f1,
                operands: vec![],
                is_return: false,
            },
            Type::I32,
        ));
        let call2 = module.expressions.alloc(Expression::new(
            ExpressionKind::Call {
                target: f2,
                operands: vec![],
                is_return: false,
            },
            Type::I32,
        ));
        // wrapper binary op (no side-effects) around the two calls
        let bin = module.expressions.alloc(Expression::new(
            ExpressionKind::Binary {
                op: crate::ir::ops::BinaryOp::AddInt32,
                left: call1,
                right: call2,
            },
            Type::I32,
        ));
        // set function body to our constructed binary
        module.get_function_mut(func_id).unwrap().set_body(bin);

        let mut pass = Vacuum;
        pass.run(&mut module);
        let body_id = module.functions[func_id].body().unwrap();
        // The body should be a block containing the LocalSet, LocalSet, and a dummy const of the binary type (i32)
        if let ExpressionKind::Block { list, .. } = &module.expressions[body_id].kind {
            assert!(matches!(
                module.expressions[list[0]].kind,
                ExpressionKind::Call { .. }
            ));
            assert!(matches!(
                module.expressions[list[1]].kind,
                ExpressionKind::Call { .. }
            ));
            assert!(matches!(
                module.expressions[list[2]].kind,
                ExpressionKind::Const(_)
            ));
        } else {
            panic!(
                "unexpected body after optimize: {:?}",
                module.expressions[body_id].kind
            );
        }
    }

    #[test]
    fn test_vacuum_drop_tee_to_set() {
        let mut module = Module::new();
        // Create a function with a local var
        let dummy = module.expressions.alloc(Expression::new(
            ExpressionKind::Const(Literal::I32(0)),
            Type::I32,
        ));
        let func_id = module.add_function(Function::new(
            "t".into(),
            Type::NONE,
            Type::I32,
            vec![Type::I32],
            dummy,
        ));
        let local = module.get_function_mut(func_id).unwrap().var_ids()[0];
        // Create a local tee and a drop of it
        let tee_val = module.expressions.alloc(Expression::new(
            ExpressionKind::Const(Literal::I32(5)),
            Type::I32,
        ));
        let tee = module.expressions.alloc(Expression::new(
            ExpressionKind::LocalTee {
                id: local,
                value: tee_val,
            },
            Type::I32,
        ));
        let drop = module.expressions.alloc(Expression::new(
            ExpressionKind::Drop { value: tee },
            Type::NONE,
        ));
        module.get_function_mut(func_id).unwrap().set_body(drop);

        let mut pass = Vacuum;
        pass.run(&mut module);
        let body_id = module.functions[func_id].body().unwrap();
        // After vacuum, the drop should be replaced by a LocalSet
        assert!(matches!(
            module.expressions[body_id].kind,
            ExpressionKind::LocalSet { .. }
        ));
    }

    #[test]
    fn test_vacuum_try_body_removed() {
        let mut module = Module::new();
        // Try whose body doesn't throw should be replaced by body
        let body = module.expressions.alloc(Expression::new(
            ExpressionKind::Const(Literal::I32(42)),
            Type::I32,
        ));
        let try_id = module.expressions.alloc(Expression::new(
            ExpressionKind::Try {
                name: None,
                body,
                catch_tags: vec![],
                catch_bodies: vec![],
                delegate: None,
            },
            Type::I32,
        ));
        let func_id = module.add_function(Function::new(
            "t".into(),
            Type::NONE,
            Type::I32,
            vec![],
            try_id,
        ));
        let mut pass = Vacuum;
        pass.run(&mut module);
        let body_id = module.functions[func_id].body().unwrap();
        assert!(matches!(
            module.expressions[body_id].kind,
            ExpressionKind::Const(_)
        ));
    }

    #[test]
    fn test_optimize_direct() {
        // Directly test `optimize` helper in isolation using calls as side-effects
        let mut module = Module::new();
        let dummy = module.expressions.alloc(Expression::new(
            ExpressionKind::Const(Literal::I32(0)),
            Type::I32,
        ));
        let _root_func = module.add_function(Function::new(
            "t".into(),
            Type::NONE,
            Type::NONE,
            vec![],
            dummy,
        ));
        let f1 = module.add_function(Function::new(
            "a".into(),
            Type::NONE,
            Type::I32,
            vec![],
            dummy,
        ));
        let f2 = module.add_function(Function::new(
            "b".into(),
            Type::NONE,
            Type::I32,
            vec![],
            dummy,
        ));
        let call1 = module.expressions.alloc(Expression::new(
            ExpressionKind::Call {
                target: f1,
                operands: vec![],
                is_return: false,
            },
            Type::I32,
        ));
        let call2 = module.expressions.alloc(Expression::new(
            ExpressionKind::Call {
                target: f2,
                operands: vec![],
                is_return: false,
            },
            Type::I32,
        ));
        let bin = module.expressions.alloc(Expression::new(
            ExpressionKind::Binary {
                op: crate::ir::ops::BinaryOp::AddInt32,
                left: call1,
                right: call2,
            },
            Type::I32,
        ));

        let mut children_with_effects: Vec<ExpressionId> = Vec::new();
        module.expressions[bin].kind.for_each_child(|child| {
            if EffectAnalyzer::analyze(child, &module.expressions).has_side_effects() {
                children_with_effects.push(child);
            }
        });
        assert_eq!(
            children_with_effects.len(),
            2,
            "expected two side-effectful children"
        );

        let mut optimizer = VacuumOptimizer {
            arena: &mut module.expressions,
            root: None,
            next_targets: Vec::new(),
        };
        if let Some(opt_id) = optimizer.optimize(bin, false, true) {
            if !matches!(
                module.expressions[opt_id].kind,
                ExpressionKind::Block { .. }
            ) {
                panic!(
                    "optimize returned unexpected kind: {:?}",
                    module.expressions[opt_id].kind
                );
            }
        } else {
            panic!("optimize returned None unexpectedly");
        }
    }
}
