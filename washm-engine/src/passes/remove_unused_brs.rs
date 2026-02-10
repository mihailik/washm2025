use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::module::Module;
use crate::ir::{Literal, Type};
use crate::passes::Pass;
use std::collections::HashSet;

/// Removes redundant branches and labels.
pub struct RemoveUnusedBrs;

impl Pass for RemoveUnusedBrs {
    fn name(&self) -> &str {
        "RemoveUnusedBrs"
    }

    fn run(&mut self, module: &mut Module) {
        let mut optimizer = BrOptimizer;
        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();

        for (_, func) in functions {
            if let Some(body) = func.body_mut() {
                optimizer.optimize(body, arena);
            }
        }
    }
}

struct Flow {
    unconditional_breaks: Vec<ExpressionId>,
    all_targets: HashSet<ExpressionId>,
    returns: Vec<ExpressionId>,
    falls_through: bool,
}

impl Flow {
    fn none() -> Self {
        Self {
            unconditional_breaks: Vec::new(),
            all_targets: HashSet::new(),
            returns: Vec::new(),
            falls_through: false,
        }
    }

    fn fallthrough() -> Self {
        Self {
            unconditional_breaks: Vec::new(),
            all_targets: HashSet::new(),
            returns: Vec::new(),
            falls_through: true,
        }
    }

    fn break_(id: ExpressionId, target: ExpressionId) -> Self {
        let mut all_targets = HashSet::new();
        all_targets.insert(target);
        Self {
            unconditional_breaks: vec![id],
            all_targets,
            returns: Vec::new(),
            falls_through: false,
        }
    }

    fn merge(&mut self, other: Flow) {
        self.unconditional_breaks.extend(other.unconditional_breaks);
        for target in other.all_targets {
            self.all_targets.insert(target);
        }
        self.returns.extend(other.returns);
        self.falls_through = self.falls_through || other.falls_through;
    }
}

struct BrOptimizer;

impl BrOptimizer {
    fn optimize(
        &mut self,
        expr_id: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) -> Flow {
        let original_id = *expr_id;
        // Collect children first to avoid borrow checker issues with the match on kind
        let kind = arena[original_id].kind.clone();

        match kind {
            ExpressionKind::Block { mut name, mut list } => {
                let mut current_flow = Flow::fallthrough();
                let mut i = 0;
                while i < list.len() {
                    let mut child_id = list[i];
                    let child_flow = self.optimize(&mut child_id, arena);
                    list[i] = child_id;

                    if !current_flow.falls_through {
                        list.truncate(i + 1);
                        break;
                    }
                    current_flow = child_flow;
                    i += 1;
                }

                let block_id = *expr_id;
                let mut still_targetted = false;
                let mut j = 0;
                while j < current_flow.unconditional_breaks.len() {
                    let br_id = current_flow.unconditional_breaks[j];
                    let is_target = if let ExpressionKind::Break {
                        target, condition, ..
                    } = arena[br_id].kind
                    {
                        target == block_id && condition.is_none()
                    } else {
                        false
                    };

                    if is_target {
                        if let ExpressionKind::Break {
                            value: Some(val), ..
                        } = arena[br_id].kind
                        {
                            arena[br_id].kind = arena[val].kind.clone();
                            arena[br_id].type_ = arena[val].type_;
                        } else {
                            arena[br_id].kind = ExpressionKind::Nop;
                            arena[br_id].type_ = Type::NONE;
                        }
                        current_flow.unconditional_breaks.remove(j);
                        current_flow.falls_through = true;
                    } else {
                        j += 1;
                    }
                }

                if current_flow.all_targets.contains(&block_id) {
                    current_flow.all_targets.remove(&block_id);
                    still_targetted = true;
                }

                if !still_targetted {
                    name = None;
                }

                // Update the block in the arena
                arena[*expr_id].kind = ExpressionKind::Block { name, list };

                current_flow
            }
            ExpressionKind::Loop { mut name, mut body } => {
                let loop_id = *expr_id;
                let mut flow = self.optimize(&mut body, arena);

                flow.unconditional_breaks.retain(|&br_id| {
                    if let ExpressionKind::Break { target, .. } = arena[br_id].kind {
                        return target != loop_id;
                    }
                    true
                });

                if flow.all_targets.contains(&loop_id) {
                    flow.all_targets.remove(&loop_id);
                } else {
                    name = None;
                }

                arena[*expr_id].kind = ExpressionKind::Loop { name, body };
                flow
            }
            ExpressionKind::If {
                name,
                mut condition,
                mut if_true,
                mut if_false,
            } => {
                let _ = self.optimize(&mut condition, arena);

                if let ExpressionKind::Const(lit) = arena[condition].kind {
                    let is_true = match lit {
                        Literal::I32(v) => v != 0,
                        Literal::I64(v) => v != 0,
                        _ => false,
                    };
                    if is_true {
                        let flow = self.optimize(&mut if_true, arena);
                        *expr_id = if_true;
                        return flow;
                    } else if let Some(mut fid) = if_false {
                        let flow = self.optimize(&mut fid, arena);
                        *expr_id = fid;
                        return flow;
                    } else {
                        arena[*expr_id].kind = ExpressionKind::Nop;
                        arena[*expr_id].type_ = Type::NONE;
                        return Flow::fallthrough();
                    }
                }

                let mut flow = self.optimize(&mut if_true, arena);
                if let Some(mut fid) = if_false {
                    let false_flow = self.optimize(&mut fid, arena);
                    flow.merge(false_flow);
                    if_false = Some(fid);
                } else {
                    flow.falls_through = true;
                }

                arena[*expr_id].kind = ExpressionKind::If {
                    name,
                    condition,
                    if_true,
                    if_false,
                };
                flow
            }
            ExpressionKind::Break {
                target,
                mut condition,
                mut value,
            } => {
                if let Some(mut cond) = condition {
                    let _ = self.optimize(&mut cond, arena);
                    if let ExpressionKind::Const(lit) = arena[cond].kind {
                        let is_true = match lit {
                            Literal::I32(v) => v != 0,
                            Literal::I64(v) => v != 0,
                            _ => false,
                        };
                        if is_true {
                            condition = None;
                        } else if let Some(mut val) = value {
                            let flow = self.optimize(&mut val, arena);
                            *expr_id = val;
                            return flow;
                        } else {
                            arena[*expr_id].kind = ExpressionKind::Nop;
                            arena[*expr_id].type_ = Type::NONE;
                            return Flow::fallthrough();
                        }
                    } else {
                        condition = Some(cond);
                    }
                }

                if let Some(mut val) = value {
                    self.optimize(&mut val, arena);
                    value = Some(val);
                }

                arena[*expr_id].kind = ExpressionKind::Break {
                    target,
                    condition,
                    value,
                };

                if condition.is_none() {
                    Flow::break_(*expr_id, target)
                } else {
                    let mut flow = Flow::fallthrough();
                    flow.all_targets.insert(target);
                    flow
                }
            }
            ExpressionKind::Switch {
                targets,
                default,
                mut condition,
                mut value,
            } => {
                self.optimize(&mut condition, arena);
                if let Some(mut val) = value {
                    self.optimize(&mut val, arena);
                    value = Some(val);
                }

                if let ExpressionKind::Const(Literal::I32(idx)) = arena[condition].kind {
                    let idx_usize = idx as usize;
                    let target = if idx_usize < targets.len() {
                        targets[idx_usize]
                    } else {
                        default
                    };
                    arena[*expr_id].kind = ExpressionKind::Break {
                        target,
                        condition: None,
                        value,
                    };
                    // re-optimize as a break
                    return self.optimize(expr_id, arena);
                }

                let mut flow = Flow::none();
                for t in targets.iter() {
                    flow.all_targets.insert(*t);
                }
                flow.all_targets.insert(default);

                arena[*expr_id].kind = ExpressionKind::Switch {
                    targets,
                    default,
                    condition,
                    value,
                };
                flow
            }
            _ => self.visit_children_flow(expr_id, arena),
        }
    }

    fn visit_children_flow(
        &mut self,
        expr_id: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) -> Flow {
        let mut current_flow = Flow::fallthrough();
        let mut kind = arena[*expr_id].kind.clone();

        match kind {
            ExpressionKind::Unary { ref mut value, .. }
            | ExpressionKind::Drop { ref mut value }
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
                current_flow = self.optimize(value, arena);
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
                current_flow = self.optimize(left, arena);
                if current_flow.falls_through {
                    let right_flow = self.optimize(right, arena);
                    current_flow.merge(right_flow);
                }
            }
            _ => {}
        }

        arena[*expr_id].kind = kind;
        current_flow
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::IrBuilder;
    use crate::ir::module::Function;
    use crate::ir::Type;

    #[test]
    fn test_remove_unused_br_void() {
        let mut module = Module::new();
        let body = {
            let mut builder = IrBuilder::new(&mut module.expressions);
            let br = builder.break_("L".to_string(), None, None, Type::NONE);
            builder.block(Some("L".to_string()), vec![br], Type::NONE)
        };

        let func = Function::new("test".to_string(), Type::NONE, Type::NONE, vec![], body);
        module.add_function(func);

        let mut pass = RemoveUnusedBrs;
        pass.run(&mut module);

        let func_id = module.functions.iter().next().unwrap().0;
        let body_id = module.functions[func_id].body().unwrap();

        if let ExpressionKind::Block { list, .. } = &module.expressions[body_id].kind {
            assert!(matches!(
                module.expressions[list[0]].kind,
                ExpressionKind::Nop
            ));
        } else {
            panic!();
        }
    }
}
