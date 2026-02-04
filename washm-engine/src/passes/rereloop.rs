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
        "rereloop"
    }

    fn run(&mut self, module: &mut Module) {
        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        for (_, func) in functions {
            if let Some(&mut body) = func.body_mut() {
                let mut context = ReReloopContext::new(arena);
                let entry = context.start_cfg_block();
                context.stack.push(Task::Triage(body));

                while let Some(task) = context.stack.pop() {
                    context.run_task(task);
                }

                // Finish current block
                if let Some(_) = context.curr_cfg_block {
                    // add return for dead ends?
                    // Binaryen does this:
                    // if (cfgBlock->BranchesOut.empty() && block->type != Type::unreachable) {
                    //   block->list.push_back(function->getResults() == Type::none ? return : unreachable)
                }

                // Run relooper
                context.relooper.calculate(entry, context.builder.arena);

                // Render
                let label_var = func.add_var(Type::I32);
                let new_body = context.relooper.render(&mut context.builder, label_var);
                func.set_body(new_body);
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
    break_targets: HashMap<String, RelooperBlockId>,
    stack: Vec<Task>,
}

impl<'a> ReReloopContext<'a> {
    fn new(arena: &'a mut IdArena<ExpressionMarker, Expression>) -> Self {
        Self {
            builder: IrBuilder::new(arena),
            relooper: Relooper::new(),
            curr_cfg_block: None,
            break_targets: HashMap::new(),
            stack: Vec::new(),
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
                let after = self.start_cfg_block();
                self.add_branch(condition_block, after, None);

                let expr = self.builder.arena.get(if_expr_id).unwrap().clone();
                if let ExpressionKind::If { if_false, .. } = &expr.kind {
                    if let Some(if_false_expr) = *if_false {
                        self.stack.push(Task::IfPhase1 {
                            _if_expr_id: if_expr_id,
                            _condition_block: condition_block,
                            if_true_end,
                        });
                        self.stack.push(Task::Triage(if_false_expr));
                    } else {
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
            ExpressionKind::Block { name, list } => {
                if let Some(ref n) = name {
                    let l = self.make_cfg_block();
                    self.break_targets.insert(n.clone(), l);
                    self.stack.push(Task::BlockJoin { later: l });
                }
                for &child in list.iter().rev() {
                    self.stack.push(Task::Triage(child));
                }
            }
            ExpressionKind::Loop { name, body } => {
                let top = self.start_cfg_block();
                if let Some(ref n) = name {
                    self.break_targets.insert(n.clone(), top);
                }
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
                name, condition, ..
            } => {
                let before = self.curr_cfg_block.unwrap();
                let target = *self
                    .break_targets
                    .get(&name)
                    .expect("Break target not found");
                self.relooper.add_branch(before, target, condition, None);
                if condition.is_some() {
                    self.start_cfg_block();
                } else {
                    self.stop_control_flow();
                }
            }
            ExpressionKind::Switch {
                names,
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

                let mut target_values: HashMap<String, Vec<u32>> = HashMap::new();
                for (i, name) in names.iter().enumerate() {
                    target_values
                        .entry(name.clone())
                        .or_default()
                        .push(i as u32);
                }

                for (name, values) in &target_values {
                    let target = *self
                        .break_targets
                        .get(name)
                        .expect("Switch target not found");
                    self.relooper
                        .add_switch_branch(before, target, values.clone(), None);
                }

                if !target_values.contains_key(&default) {
                    let target = *self
                        .break_targets
                        .get(&default)
                        .expect("Switch default not found");
                    self.relooper
                        .add_switch_branch(before, target, Vec::new(), None);
                } else {
                    let temp = self.start_cfg_block();
                    self.relooper
                        .add_switch_branch(before, temp, Vec::new(), None);
                    let target = *self
                        .break_targets
                        .get(&default)
                        .expect("Switch default not found");
                    self.add_branch(temp, target, None);
                }

                self.stop_control_flow();
            }
            ExpressionKind::Return { .. } | ExpressionKind::Unreachable => {
                self.get_curr_block_list_mut().push(expr_id);
                self.stop_control_flow();
            }
            _ => {
                self.get_curr_block_list_mut().push(expr_id);
            }
        }
    }
}
