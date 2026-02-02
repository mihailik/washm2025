use crate::analysis::effects::EffectAnalyzer;
use crate::ir::expression::{ExprRef, Expression, ExpressionKind};
use crate::ir::module::Module;
use crate::ir::{Literal, Type};
use crate::passes::Pass;
use bumpalo::collections::Vec as BumpVec;

/// Removes redundant code such as nops, unused expressions, and simplifies blocks/ifs.
pub struct Vacuum;

impl Pass for Vacuum {
    fn name(&self) -> &str {
        "Vacuum"
    }

    fn run(&mut self, module: &mut Module) {
        let mut optimizer = VacuumOptimizer {
            bump: module.allocator,
        };
        for (_, func) in &mut module.functions {
            if let Some(mut body) = func.body() {
                optimizer.process(&mut body);
                if let Some(mut_body) = func.body_mut() {
                    *mut_body = body;
                }
            }
        }
    }
}

struct VacuumOptimizer<'a> {
    bump: &'a bumpalo::Bump,
}

impl<'a> VacuumOptimizer<'a> {
    fn process(&mut self, expr: &mut ExprRef<'a>) {
        let block_type = expr.type_;
        match &mut expr.kind {
            ExpressionKind::Block { name, list } => {
                for child in list.iter_mut() {
                    self.process(child);
                }

                let mut new_list = BumpVec::new_in(self.bump);
                let list_len = list.len();
                let mut i = 0;
                while i < list_len {
                    let child = list[i];
                    if let ExpressionKind::Block {
                        name: None,
                        list: child_list,
                    } = &child.kind
                    {
                        for &cc in child_list {
                            self.add_to_block(&mut new_list, cc, block_type, i == list_len - 1);
                        }
                    } else {
                        self.add_to_block(&mut new_list, child, block_type, i == list_len - 1);
                    }
                    if child.type_ == Type::UNREACHABLE {
                        break;
                    }
                    i += 1;
                }
                *list = new_list;

                if name.is_none() {
                    if list.is_empty() {
                        **expr = Expression {
                            kind: ExpressionKind::Nop,
                            type_: Type::NONE,
                        };
                    } else if list.len() == 1 {
                        let child = list[0];
                        unsafe {
                            std::ptr::swap(expr.as_ptr(), child.as_ptr());
                        }
                    }
                }
            }
            ExpressionKind::If {
                condition,
                if_true,
                if_false,
            } => {
                self.process(condition);
                self.process(if_true);
                if let Some(f) = if_false {
                    self.process(f);
                }

                if let ExpressionKind::Const(lit) = &condition.kind {
                    let is_true = match lit {
                        Literal::I32(v) => *v != 0,
                        Literal::I64(v) => *v != 0,
                        _ => false,
                    };
                    if is_true {
                        let true_branch = *if_true;
                        unsafe {
                            std::ptr::swap(expr.as_ptr(), true_branch.as_ptr());
                        }
                    } else if let Some(false_branch) = *if_false {
                        unsafe {
                            std::ptr::swap(expr.as_ptr(), false_branch.as_ptr());
                        }
                    } else {
                        **expr = Expression {
                            kind: ExpressionKind::Nop,
                            type_: Type::NONE,
                        };
                    }
                    return;
                }

                if let Some(f) = if_false {
                    if matches!(f.kind, ExpressionKind::Nop) {
                        *if_false = None;
                    }
                }

                if matches!(if_true.kind, ExpressionKind::Nop) {
                    if let Some(f) = if_false.take() {
                        *if_true = f;
                    }
                }

                if matches!(if_true.kind, ExpressionKind::Nop) && if_false.is_none() {
                    let cond = *condition;
                    **expr = Expression {
                        kind: ExpressionKind::Drop { value: cond },
                        type_: Type::NONE,
                    };
                    self.process(expr);
                }
            }
            ExpressionKind::Drop { value } => {
                self.process(value);
                if !EffectAnalyzer::analyze(*value).has_side_effects() {
                    **expr = Expression {
                        kind: ExpressionKind::Nop,
                        type_: Type::NONE,
                    };
                }
            }
            ExpressionKind::Loop { body, .. } => {
                self.process(body);
            }
            _ => self.visit_children(expr),
        }
    }

    fn add_to_block(
        &self,
        list: &mut BumpVec<'a, ExprRef<'a>>,
        child: ExprRef<'a>,
        block_type: Type,
        is_last: bool,
    ) {
        if matches!(child.kind, ExpressionKind::Nop) {
            if is_last && block_type != Type::NONE && block_type != Type::UNREACHABLE {
                list.push(child);
            }
            return;
        }
        if !is_last
            && child.type_ != Type::UNREACHABLE
            && !EffectAnalyzer::analyze(child).has_side_effects()
        {
            return;
        }
        list.push(child);
    }

    fn visit_children(&mut self, expr: &mut ExprRef<'a>) {
        match &mut expr.kind {
            ExpressionKind::Unary { value, .. }
            | ExpressionKind::LocalSet { value, .. }
            | ExpressionKind::LocalTee { value, .. }
            | ExpressionKind::GlobalSet { value, .. }
            | ExpressionKind::Load { ptr: value, .. }
            | ExpressionKind::MemoryGrow { delta: value, .. } => {
                self.process(value);
            }
            ExpressionKind::Binary { left, right, .. }
            | ExpressionKind::Store {
                ptr: left,
                value: right,
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
                if_true,
                if_false,
                condition,
                ..
            } => {
                self.process(if_true);
                self.process(if_false);
                self.process(condition);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::{ExprRef, Expression, ExpressionKind};
    use crate::ir::module::Function;
    use crate::ir::{Literal, Type};
    use bumpalo::collections::Vec as BumpVec;
    use bumpalo::Bump;

    #[test]
    fn test_vacuum_block_flatten() {
        let bump = Bump::new();
        let mut inner_list = BumpVec::new_in(&bump);
        inner_list.push(ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(42)),
            type_: Type::I32,
        })));
        let inner_block = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Block {
                name: None,
                list: inner_list,
            },
            type_: Type::I32,
        }));

        let mut outer_list = BumpVec::new_in(&bump);
        outer_list.push(inner_block);
        let outer_block = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Block {
                name: None,
                list: outer_list,
            },
            type_: Type::I32,
        }));

        let mut module = Module::new(&bump);
        let func_id = module.add_function(Function::new(
            "t".into(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(outer_block),
        ));

        let mut pass = Vacuum;
        pass.run(&mut module);

        let binding = module.functions[func_id].body();
        let body = binding.as_ref().unwrap();
        assert!(matches!(body.kind, ExpressionKind::Const(_)));
    }
}
