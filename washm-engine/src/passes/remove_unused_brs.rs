use crate::ir::expression::{ExprRef, Expression, ExpressionKind};
use crate::ir::module::Module;
use crate::passes::Pass;
use crate::ir::{Literal, Type};
use std::collections::HashSet;

/// Removes redundant branches and labels.
pub struct RemoveUnusedBrs;

impl Pass for RemoveUnusedBrs {
    fn name(&self) -> &str {
        "RemoveUnusedBrs"
    }

    fn run(&mut self, module: &mut Module) {
        let mut optimizer = BrOptimizer {
            _bump: module.allocator,
        };

        for func in &mut module.functions {
            if let Some(mut body) = func.body {
                optimizer.optimize(&mut body);
                func.body = Some(body);
            }
        }
    }
}

struct Flow<'a> {
    unconditional_breaks: Vec<ExprRef<'a>>,
    all_targets: HashSet<&'a str>,
    returns: Vec<ExprRef<'a>>,
    falls_through: bool,
}

impl<'a> Flow<'a> {
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

    fn break_(expr: ExprRef<'a>, name: &'a str) -> Self {
        let mut all_targets = HashSet::new();
        all_targets.insert(name);
        Self {
            unconditional_breaks: vec![expr],
            all_targets,
            returns: Vec::new(),
            falls_through: false,
        }
    }

    #[allow(dead_code)]
    fn return_(expr: ExprRef<'a>) -> Self {
        Self {
            unconditional_breaks: Vec::new(),
            all_targets: HashSet::new(),
            returns: vec![expr],
            falls_through: false,
        }
    }

    fn merge(&mut self, other: Flow<'a>) {
        self.unconditional_breaks.extend(other.unconditional_breaks);
        for target in other.all_targets {
            self.all_targets.insert(target);
        }
        self.returns.extend(other.returns);
        self.falls_through = self.falls_through || other.falls_through;
    }
}

struct BrOptimizer<'a> {
    _bump: &'a bumpalo::Bump,
}

impl<'a> BrOptimizer<'a> {
    fn optimize(&mut self, expr: &mut ExprRef<'a>) -> Flow<'a> {
        let e = *expr;
        match &mut expr.kind {
            ExpressionKind::Block { name, list } => {
                let mut current_flow = Flow::fallthrough();
                let mut i = 0;
                while i < list.len() {
                    let child_flow = self.optimize(&mut list[i]);
                    if !current_flow.falls_through {
                        list.truncate(i);
                        break;
                    }
                    current_flow = child_flow;
                    i += 1;
                }

                if !current_flow.falls_through && i < list.len() {
                    list.truncate(i);
                }

                if let Some(block_name) = name {
                    let mut still_targetted = false;
                    let mut j = 0;
                    while j < current_flow.unconditional_breaks.len() {
                        let mut br = current_flow.unconditional_breaks[j];
                        let is_target = if let ExpressionKind::Break {
                            name: br_name,
                            condition,
                            ..
                        } = &br.kind
                        {
                            *br_name == *block_name && condition.is_none()
                        } else {
                            false
                        };

                        if is_target {
                            if let ExpressionKind::Break { value, .. } = &br.kind {
                                if let Some(val) = value {
                                    let val_ref = *val;
                                    unsafe {
                                        let val_ptr = val_ref.as_ptr();
                                        let br_ptr = br.as_ptr();
                                        std::ptr::swap(br_ptr, val_ptr);
                                    }
                                } else {
                                    *br = Expression {
                                        kind: ExpressionKind::Nop,
                                        type_: Type::NONE,
                                    };
                                }
                            }
                            current_flow.unconditional_breaks.remove(j);
                            current_flow.falls_through = true;
                        } else {
                            still_targetted = true;
                            j += 1;
                        }
                    }

                    if !still_targetted && !current_flow.all_targets.contains(block_name) {
                        *name = None;
                    } else {
                        current_flow.all_targets.remove(block_name);
                    }
                }

                current_flow
            }
            ExpressionKind::Loop { name, body } => {
                let mut flow = self.optimize(body);

                if let Some(loop_name) = name {
                    flow.unconditional_breaks.retain(|br| {
                        if let ExpressionKind::Break { name: br_name, .. } = &br.kind {
                            return *br_name != *loop_name;
                        }
                        true
                    });

                    if !flow.all_targets.contains(loop_name) {
                        *name = None;
                    } else {
                        flow.all_targets.remove(loop_name);
                    }
                }
                flow
            }
            ExpressionKind::If {
                condition,
                if_true,
                if_false,
            } => {
                let _ = self.optimize(condition);

                if let ExpressionKind::Const(lit) = &condition.kind {
                    let is_true = match lit {
                        Literal::I32(v) => *v != 0,
                        Literal::I64(v) => *v != 0,
                        _ => false,
                    };
                    if is_true {
                        let flow = self.optimize(if_true);
                        let true_ref = *if_true;
                        unsafe {
                            std::ptr::swap(expr.as_ptr(), true_ref.as_ptr());
                        }
                        return flow;
                    } else if let Some(f_expr) = if_false {
                        let flow = self.optimize(f_expr);
                        let false_ref = *f_expr;
                        unsafe {
                            std::ptr::swap(expr.as_ptr(), false_ref.as_ptr());
                        }
                        return flow;
                    } else {
                        **expr = Expression {
                            kind: ExpressionKind::Nop,
                            type_: Type::NONE,
                        };
                        return Flow::fallthrough();
                    }
                }

                let mut flow = self.optimize(if_true);
                if let Some(f_expr) = if_false {
                    let false_flow = self.optimize(f_expr);
                    flow.merge(false_flow);
                } else {
                    flow.falls_through = true;
                }
                flow
            }
            ExpressionKind::Break {
                name,
                condition,
                value,
            } => {
                let br_name = *name;
                if let Some(cond) = condition {
                    let _ = self.optimize(cond);
                    if let ExpressionKind::Const(lit) = &cond.kind {
                        let is_true = match lit {
                            Literal::I32(v) => *v != 0,
                            Literal::I64(v) => *v != 0,
                            _ => false,
                        };
                        if is_true {
                            *condition = None;
                        } else {
                            if let Some(val) = value {
                                let val_ref = *val;
                                unsafe {
                                    std::ptr::swap(expr.as_ptr(), val_ref.as_ptr());
                                }
                                return self.optimize(expr);
                            } else {
                                **expr = Expression {
                                    kind: ExpressionKind::Nop,
                                    type_: Type::NONE,
                                };
                                return Flow::fallthrough();
                            }
                        }
                    }
                }

                if let Some(val) = value {
                    self.optimize(val);
                }

                if condition.is_none() {
                    Flow::break_(e, br_name)
                } else {
                    let mut flow = Flow::fallthrough();
                    flow.all_targets.insert(br_name);
                    flow
                }
            }
            ExpressionKind::Switch {
                names,
                default,
                condition,
                value,
            } => {
                self.optimize(condition);
                if let Some(val) = value {
                    self.optimize(val);
                }

                if let ExpressionKind::Const(Literal::I32(idx)) = &condition.kind {
                    let idx_usize = *idx as usize;
                    let target = if idx_usize < names.len() {
                        names[idx_usize]
                    } else {
                        default
                    };
                    expr.kind = ExpressionKind::Break {
                        name: target,
                        condition: None,
                        value: *value,
                    };
                    return self.optimize(expr);
                }

                let mut flow = Flow::none();
                for &n in names.iter() {
                    flow.all_targets.insert(n);
                }
                flow.all_targets.insert(default);
                flow
            }
            _ => self.visit_children_flow(expr),
        }
    }

    fn visit_children_flow(&mut self, expr: &mut ExprRef<'a>) -> Flow<'a> {
        let mut current_flow = Flow::fallthrough();
        match &mut expr.kind {
            ExpressionKind::Unary { value, .. }
            | ExpressionKind::Drop { value }
            | ExpressionKind::LocalSet { value, .. }
            | ExpressionKind::LocalTee { value, .. }
            | ExpressionKind::GlobalSet { value, .. }
            | ExpressionKind::Load { ptr: value, .. }
            | ExpressionKind::MemoryGrow { delta: value } => {
                current_flow = self.optimize(value);
            }
            ExpressionKind::Binary { left, right, .. }
            | ExpressionKind::Store {
                ptr: left,
                value: right,
                ..
            } => {
                current_flow = self.optimize(left);
                if current_flow.falls_through {
                    current_flow = self.optimize(right);
                }
            }
            _ => {}
        }
        current_flow
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::{ExprRef, Expression, ExpressionKind};
    use crate::ir::module::Function;
    use crate::ir::Type;
    use bumpalo::collections::Vec as BumpVec;
    use bumpalo::Bump;

    #[test]
    fn test_remove_unused_br_void() {
        let bump = Bump::new();
        let br_expr = bump.alloc(Expression {
            kind: ExpressionKind::Break {
                name: "L",
                condition: None,
                value: None,
            },
            type_: Type::UNREACHABLE,
        });
        let br = ExprRef::new(br_expr);
        let mut list = BumpVec::new_in(&bump);
        list.push(br);
        let block_expr = bump.alloc(Expression {
            kind: ExpressionKind::Block {
                name: Some("L"),
                list,
            },
            type_: Type::NONE,
        });
        let block = ExprRef::new(block_expr);
        let func = Function::new(
            "test".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(block),
        );
        let mut module = Module::new(&bump);
        module.add_function(func);
        let mut pass = RemoveUnusedBrs;
        pass.run(&mut module);
        let body = module.functions[0].body.as_ref().unwrap();
        if let ExpressionKind::Block { list, .. } = &body.kind {
            assert!(matches!(list[0].kind, ExpressionKind::Nop));
        } else {
            panic!();
        }
    }
}
