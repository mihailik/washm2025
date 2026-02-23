use crate::cfg::relooper::{Relooper, RelooperBlockId, RelooperBranchId};
use crate::collections::id_arena::IdArena;
use crate::ir::expression::{
    Expression, ExpressionId, ExpressionKind, ExpressionMarker, IrBuilder,
};
use crate::ir::module::Module;
use crate::ir::Type;
use crate::passes::Pass;
use std::collections::HashMap;

pub struct Rereloop;

impl Pass for Rereloop {
    fn name(&self) -> &str {
        "Rereloop"
    }

    fn run(&mut self, module: &mut Module) {
        println!("PASS: Rereloop");
        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        for (_, func) in functions {
            if let crate::ir::module::FunctionKind::Local { body, .. } = &func.kind {
                let mut context = ReReloopContext::new(arena);
                let body_id = *body;

                // Pre-pass: Find all named blocks/loops and create CFG blocks for them
                // so that Breaks can always find their targets regardless of traversal order.
                context.pre_scan_labels(body_id);

                let entry = context.start_cfg_block();
                context.stack.push(Task::Triage(body_id));

                while let Some(task) = context.stack.pop() {
                    context.run_task(task);
                }

                // Finish current block
                if let Some(curr) = context.curr_cfg_block {
                    // If the function reaches the end, connect to exit_block
                    let exit = context.get_exit_block();
                    context.add_branch(curr, exit, None);
                }

                // Run relooper
                context.relooper.calculate(entry, context.builder.arena);

                // Render
                let label_var = func.add_var(Type::I32);
                let new_body = context.relooper.render(&mut context.builder, label_var);

                // Update body
                if let crate::ir::module::FunctionKind::Local { body, .. } = &mut func.kind {
                    *body = new_body;
                }
            }
        }
    }
}

enum Task {
    Triage(ExpressionId),
    BlockJoin {
        later: RelooperBlockId,
    },
    IfPhase0 {
        if_expr_id: ExpressionId,
        condition_block: RelooperBlockId,
    },
    IfPhase1 {
        _if_expr_id: ExpressionId,
        _condition_block: RelooperBlockId,
        if_true_end: RelooperBlockId,
    },
}

struct ReReloopContext<'a> {
    builder: IrBuilder<'a>,
    relooper: Relooper,
    curr_cfg_block: Option<RelooperBlockId>,
    break_targets: HashMap<ExpressionId, RelooperBlockId>,
    label_counts: HashMap<ExpressionId, usize>,
    stack: Vec<Task>,
    exit_block: Option<RelooperBlockId>,
}

impl<'a> ReReloopContext<'a> {
    fn new(arena: &'a mut IdArena<ExpressionMarker, Expression>) -> Self {
        Self {
            builder: IrBuilder::new(arena),
            relooper: Relooper::new(),
            curr_cfg_block: None,
            break_targets: HashMap::new(),
            label_counts: HashMap::new(),
            stack: Vec::new(),
            exit_block: None,
        }
    }

    fn get_exit_block(&mut self) -> RelooperBlockId {
        if let Some(e) = self.exit_block {
            e
        } else {
            let e = self.make_cfg_block();
            self.exit_block = Some(e);
            e
        }
    }

    fn make_cfg_block(&mut self) -> RelooperBlockId {
        let block_expr = self
            .builder
            .block(None as Option<&str>, Vec::new(), Type::NONE);
        self.relooper.add_block(block_expr, None)
    }

    fn set_curr_cfg_block(&mut self, curr: RelooperBlockId) -> RelooperBlockId {
        self.curr_cfg_block = Some(curr);
        curr
    }

    fn start_cfg_block(&mut self) -> RelooperBlockId {
        let block = self.make_cfg_block();
        self.set_curr_cfg_block(block)
    }

    fn pre_scan_labels(&mut self, expr_id: ExpressionId) {
        let mut stack = vec![expr_id];
        let mut visited = std::collections::HashSet::new();

        while let Some(curr_id) = stack.pop() {
            if !visited.insert(curr_id) {
                continue;
            }

            let expr_kind = self.builder.arena.get(curr_id).unwrap().kind.clone();
            match &expr_kind {
                ExpressionKind::Block { .. }
                | ExpressionKind::Loop { .. }
                | ExpressionKind::If { .. } => {
                    let l = self.make_cfg_block();
                    self.break_targets.insert(curr_id, l);
                }
                _ => {}
            }
            match &expr_kind {
                ExpressionKind::Break { target, .. } => {
                    *self.label_counts.entry(*target).or_insert(0) += 1;
                }
                ExpressionKind::Switch {
                    targets, default, ..
                } => {
                    for t in targets {
                        *self.label_counts.entry(*t).or_insert(0) += 1;
                    }
                    *self.label_counts.entry(*default).or_insert(0) += 1;
                }
                _ => {}
            }
            expr_kind.for_each_child(|child| {
                stack.push(child);
            });
        }
    }

    fn get_curr_block_list_mut(&mut self) -> &mut Vec<ExpressionId> {
        let block_id = self.curr_cfg_block.unwrap();
        let expr_id = self.relooper.blocks.get(block_id).unwrap().code;
        let expr = self.builder.arena.get_mut(expr_id).unwrap();
        if let ExpressionKind::Block { list, .. } = &mut expr.kind {
            list
        } else {
            panic!("CFG block code must be a Block expression");
        }
    }

    fn add_branch(
        &mut self,
        from: RelooperBlockId,
        to: RelooperBlockId,
        condition: Option<ExpressionId>,
    ) -> RelooperBranchId {
        self.relooper.add_branch(from, to, condition, None)
    }

    fn stop_control_flow(&mut self) {
        self.start_cfg_block();
    }

    fn run_task(&mut self, task: Task) {
        match task {
            Task::Triage(expr_id) => self.triage(expr_id),
            Task::BlockJoin { later } => {
                let curr = self.curr_cfg_block.unwrap();
                self.add_branch(curr, later, None);
                self.set_curr_cfg_block(later);
            }
            Task::IfPhase0 {
                if_expr_id,
                condition_block,
            } => {
                let if_true_end = self.curr_cfg_block.unwrap();
                let expr = self.builder.arena.get(if_expr_id).unwrap().clone();
                if let ExpressionKind::If { if_false, .. } = &expr.kind {
                    if let Some(if_false_expr) = *if_false {
                        let if_false_begin = self.start_cfg_block();
                        self.add_branch(condition_block, if_false_begin, None);
                        self.stack.push(Task::IfPhase1 {
                            _if_expr_id: if_expr_id,
                            _condition_block: condition_block,
                            if_true_end,
                        });
                        self.stack.push(Task::Triage(if_false_expr));
                    } else {
                        let after = self.start_cfg_block();
                        self.add_branch(condition_block, after, None);
                        self.add_branch(if_true_end, after, None);
                    }
                }
            }
            Task::IfPhase1 { if_true_end, .. } => {
                let if_false_end = self.curr_cfg_block.unwrap();
                let after = self.start_cfg_block();
                self.add_branch(if_true_end, after, None);
                self.add_branch(if_false_end, after, None);
            }
        }
    }

    fn triage(&mut self, expr_id: ExpressionId) {
        let expr_kind = self.builder.arena.get(expr_id).unwrap().kind.clone();
        match expr_kind {
            ExpressionKind::Block { list, .. } => {
                let count = self.label_counts.get(&expr_id).cloned().unwrap_or(0);
                if count > 0 {
                    let l = *self.break_targets.get(&expr_id).unwrap();
                    self.stack.push(Task::BlockJoin { later: l });
                }
                for &child in list.iter().rev() {
                    self.stack.push(Task::Triage(child));
                }
            }
            ExpressionKind::Loop { body, .. } => {
                let before = self.curr_cfg_block.unwrap();
                let top = if let Some(&t) = self.break_targets.get(&expr_id) {
                    t
                } else {
                    self.start_cfg_block()
                };
                self.set_curr_cfg_block(top);
                self.add_branch(before, top, None);

                let after = self.make_cfg_block();
                self.stack.push(Task::BlockJoin { later: after });
                self.stack.push(Task::Triage(body));
            }
            ExpressionKind::If {
                condition, if_true, ..
            } => {
                let condition_block = self.curr_cfg_block.unwrap();
                let if_true_begin = self.start_cfg_block();
                self.add_branch(condition_block, if_true_begin, Some(condition));

                self.stack.push(Task::IfPhase0 {
                    if_expr_id: expr_id,
                    condition_block,
                });
                self.stack.push(Task::Triage(if_true));
            }
            ExpressionKind::Break {
                target, condition, ..
            } => {
                let target_id = target;
                let before = self.curr_cfg_block.unwrap();
                let target = match self.break_targets.get(&target_id) {
                    Some(&t) => t,
                    None => self.get_exit_block(),
                };
                if let Some(cond) = condition {
                    let next = self.make_cfg_block();
                    self.relooper.add_branch(before, target, Some(cond), None);
                    self.relooper.add_branch(before, next, None, None);
                    self.set_curr_cfg_block(next);
                } else {
                    self.relooper.add_branch(before, target, None, None);
                    self.stop_control_flow();
                }
            }
            ExpressionKind::Switch {
                targets,
                default,
                condition,
                ..
            } => {
                let before = self.curr_cfg_block.unwrap();
                {
                    let block = self.relooper.blocks.get_mut(before).unwrap();
                    assert!(block.switch_condition.is_none());
                    block.switch_condition = Some(condition);
                }

                let mut target_values: HashMap<ExpressionId, Vec<u32>> = HashMap::new();
                for (i, t) in targets.iter().enumerate() {
                    target_values.entry(*t).or_default().push(i as u32);
                }

                for (t_id, values) in &target_values {
                    let target = match self.break_targets.get(t_id) {
                        Some(&t) => t,
                        None => self.get_exit_block(),
                    };
                    self.relooper
                        .add_switch_branch(before, target, values.clone(), None);
                }

                let default_target = match self.break_targets.get(&default) {
                    Some(&t) => t,
                    None => self.get_exit_block(),
                };

                if !target_values.contains_key(&default) {
                    self.relooper
                        .add_switch_branch(before, default_target, Vec::new(), None);
                } else {
                    let temp = self.start_cfg_block();
                    self.relooper
                        .add_switch_branch(before, temp, Vec::new(), None);
                    self.relooper.add_branch(temp, default_target, None, None);
                }

                self.stop_control_flow();
            }
            ExpressionKind::Return { .. } | ExpressionKind::Unreachable => {
                self.get_curr_block_list_mut().push(expr_id);
                self.stop_control_flow();
            }
            _ => {
                let list = self.get_curr_block_list_mut();
                list.push(expr_id);
            }
        }
    }
}
