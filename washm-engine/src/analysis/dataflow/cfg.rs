use crate::analysis::dataflow::liveness::{ActionKind, InterferenceGraph, LivenessAction};
use crate::ir::expression::{ExprRef, Expression, ExpressionKind};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct BasicBlock<'a> {
    pub index: usize,
    pub preds: Vec<usize>,
    pub succs: Vec<usize>,
    pub actions: Vec<LivenessAction<'a>>,
    pub live_in: HashSet<u32>,
    pub live_out: HashSet<u32>,
}

impl<'a> BasicBlock<'a> {
    pub fn new(index: usize) -> Self {
        Self {
            index,
            preds: Vec::new(),
            succs: Vec::new(),
            actions: Vec::new(),
            live_in: HashSet::new(),
            live_out: HashSet::new(),
        }
    }
}

pub struct ControlFlowGraph<'a> {
    pub blocks: Vec<BasicBlock<'a>>,
    pub entry: usize,
}

impl<'a> Default for ControlFlowGraph<'a> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> ControlFlowGraph<'a> {
    pub fn new() -> Self {
        Self {
            blocks: Vec::new(),
            entry: 0,
        }
    }

    pub fn add_block(&mut self) -> usize {
        let index = self.blocks.len();
        self.blocks.push(BasicBlock::new(index));
        index
    }

    pub fn add_edge(&mut self, from: usize, to: usize) {
        if !self.blocks[from].succs.contains(&to) {
            self.blocks[from].succs.push(to);
        }
        if !self.blocks[to].preds.contains(&from) {
            self.blocks[to].preds.push(from);
        }
    }

    pub fn calculate_liveness(&mut self) {
        let mut changed = true;
        while changed {
            changed = false;
            for i in (0..self.blocks.len()).rev() {
                let mut new_live_out = HashSet::new();
                for &succ_idx in &self.blocks[i].succs {
                    new_live_out.extend(&self.blocks[succ_idx].live_in);
                }

                if new_live_out != self.blocks[i].live_out {
                    self.blocks[i].live_out = new_live_out.clone();
                    changed = true;
                }

                let mut current_live = self.blocks[i].live_out.clone();
                for action in self.blocks[i].actions.iter().rev() {
                    match action.kind {
                        ActionKind::Set => {
                            current_live.remove(&action.index);
                        }
                        ActionKind::Get => {
                            current_live.insert(action.index);
                        }
                        _ => {}
                    }
                }

                if current_live != self.blocks[i].live_in {
                    self.blocks[i].live_in = current_live;
                    changed = true;
                }
            }
        }
    }

    pub fn calculate_interference(&mut self) -> InterferenceGraph {
        let mut graph = InterferenceGraph::new();
        let mut value_counter = 0;
        let mut next_value = || {
            value_counter += 1;
            value_counter
        };

        for block_idx in 0..self.blocks.len() {
            let block = &mut self.blocks[block_idx];
            // 1. Backward pass
            let mut live = block.live_out.clone();
            let mut ends_live_range = vec![false; block.actions.len()];

            for i in (0..block.actions.len()).rev() {
                let action = &mut block.actions[i];
                if action.is_get() {
                    if !live.contains(&action.index) {
                        ends_live_range[i] = true;
                        live.insert(action.index);
                    }
                } else if action.is_set() && live.remove(&action.index) {
                    action.effective = true;
                }
            }

            // 2. Forward pass
            let mut current_values = HashMap::new();
            for &local in &block.live_in {
                current_values.insert(local, next_value());
            }

            let mut live = block.live_in.clone();

            for (i, _action) in block.actions.iter().enumerate() {
                let action_ref = &block.actions[i]; // Immutable ref first
                let index = action_ref.index;
                let is_get = action_ref.is_get();
                let is_set = action_ref.is_set();
                let effective = action_ref.effective;
                let copy_from = action_ref.copy_from;

                if is_get {
                    if ends_live_range[i] {
                        live.remove(&index);
                    }
                    continue;
                }

                if is_set {
                    let new_val = if let Some(src) = copy_from {
                        *current_values.get(&src).unwrap_or(&next_value())
                    } else {
                        next_value()
                    };

                    current_values.insert(index, new_val);

                    if !effective {
                        continue;
                    }

                    for &other in &live {
                        if other != index {
                            let other_val = current_values.get(&other).unwrap_or(&0);
                            if *other_val != new_val {
                                graph.add(index, other);
                            }
                        }
                    }

                    live.insert(index);
                }
            }
        }
        graph
    }
}

#[derive(Clone)]
struct Scope {
    label: Option<String>,
    break_target: usize,
    continue_target: Option<usize>,
}

pub struct CFGBuilder<'a> {
    cfg: ControlFlowGraph<'a>,
    current_block: usize,
    scope_stack: Vec<Scope>,
    _marker: std::marker::PhantomData<&'a mut Expression<'a>>,
}

impl<'a> Default for CFGBuilder<'a> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> CFGBuilder<'a> {
    pub fn new() -> Self {
        let mut cfg = ControlFlowGraph::new();
        let entry = cfg.add_block();
        cfg.entry = entry;
        Self {
            cfg,
            current_block: entry,
            scope_stack: Vec::new(),
            _marker: std::marker::PhantomData,
        }
    }

    pub fn build(mut self, root: &mut Expression<'a>) -> ControlFlowGraph<'a> {
        self.visit(root);
        self.cfg
    }

    fn make_ref(expr: &mut Expression<'a>) -> ExprRef<'a> {
        unsafe {
            let ptr: *mut Expression<'a> = expr as *mut _;
            let unbounded: &'a mut Expression<'a> = &mut *ptr;
            ExprRef::new(unbounded)
        }
    }

    fn visit(&mut self, expr: &mut Expression<'a>) {
        let expr_ptr = Self::make_ref(expr);

        match &mut expr.kind {
            ExpressionKind::Block { name, list } => {
                let end_block = self.cfg.add_block();
                let label = name.map(|s| s.to_string());

                self.scope_stack.push(Scope {
                    label,
                    break_target: end_block,
                    continue_target: None,
                });

                for child in list {
                    self.visit(child);
                }

                self.scope_stack.pop();
                self.cfg.add_edge(self.current_block, end_block);
                self.current_block = end_block;
            }
            ExpressionKind::Loop { name, body } => {
                let loop_head = self.cfg.add_block();
                let loop_exit = self.cfg.add_block();
                let label = name.map(|s| s.to_string());

                self.cfg.add_edge(self.current_block, loop_head);
                self.current_block = loop_head;

                self.scope_stack.push(Scope {
                    label,
                    break_target: loop_exit,
                    continue_target: Some(loop_head),
                });

                self.visit(body);

                self.scope_stack.pop();

                self.cfg.add_edge(self.current_block, loop_exit);
                self.current_block = loop_exit;
            }
            ExpressionKind::If {
                condition,
                if_true,
                if_false,
            } => {
                self.visit(condition);

                let condition_block = self.current_block;
                let true_block = self.cfg.add_block();
                let merge_block = self.cfg.add_block();
                let false_block = if if_false.is_some() {
                    self.cfg.add_block()
                } else {
                    merge_block
                };

                self.cfg.add_edge(condition_block, true_block);
                self.cfg.add_edge(condition_block, false_block);

                self.current_block = true_block;
                self.visit(if_true);
                self.cfg.add_edge(self.current_block, merge_block);

                if let Some(false_expr) = if_false {
                    self.current_block = false_block;
                    self.visit(false_expr);
                    self.cfg.add_edge(self.current_block, merge_block);
                }

                self.current_block = merge_block;
            }
            ExpressionKind::Break {
                name,
                condition,
                value,
            } => {
                if let Some(v) = value {
                    self.visit(v);
                }
                if let Some(c) = condition {
                    self.visit(c);
                    if let Some(target) = self.find_break_target(Some(*name)) {
                        self.cfg.add_edge(self.current_block, target);
                    }
                } else {
                    if let Some(target) = self.find_break_target(Some(*name)) {
                        self.cfg.add_edge(self.current_block, target);
                    }
                    let dead = self.cfg.add_block();
                    self.current_block = dead;
                }
            }
            ExpressionKind::LocalGet { index } => {
                self.cfg.blocks[self.current_block]
                    .actions
                    .push(LivenessAction::get(*index, expr_ptr));
            }
            ExpressionKind::LocalSet { index, value } => {
                let copy_from = if let ExpressionKind::LocalGet { index: src_idx } = &value.kind {
                    Some(*src_idx)
                } else {
                    None
                };
                self.visit(value);
                self.cfg.blocks[self.current_block]
                    .actions
                    .push(LivenessAction::set(*index, expr_ptr, copy_from));
            }
            ExpressionKind::LocalTee { index, value } => {
                let copy_from = if let ExpressionKind::LocalGet { index: src_idx } = &value.kind {
                    Some(*src_idx)
                } else {
                    None
                };
                self.visit(value);
                self.cfg.blocks[self.current_block]
                    .actions
                    .push(LivenessAction::set(*index, expr_ptr, copy_from));
            }
            ExpressionKind::Return { value } => {
                if let Some(v) = value {
                    self.visit(v);
                }
                let dead = self.cfg.add_block();
                self.current_block = dead;
            }
            ExpressionKind::Call { operands, .. }
            | ExpressionKind::CallIndirect { operands, .. } => {
                for op in operands {
                    self.visit(op);
                }
            }
            ExpressionKind::Drop { value } => self.visit(value),
            ExpressionKind::Unary { value, .. } => self.visit(value),
            ExpressionKind::Binary { left, right, .. } => {
                self.visit(left);
                self.visit(right);
            }
            ExpressionKind::Const(_) | ExpressionKind::Nop | ExpressionKind::Unreachable => {}
            _ => {}
        }
    }

    fn find_break_target(&self, name: Option<&str>) -> Option<usize> {
        if let Some(n) = name {
            for scope in self.scope_stack.iter().rev() {
                if scope.label.as_deref() == Some(n) {
                    return scope.continue_target.or(Some(scope.break_target));
                }
            }
        } else if let Some(scope) = self.scope_stack.last() {
            return scope.continue_target.or(Some(scope.break_target));
        }
        None
    }
}
