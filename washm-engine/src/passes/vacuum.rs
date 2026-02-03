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
        let mut optimizer = VacuumOptimizer { arena };
        for (_, func) in functions.iter_mut() {
            if let Some(body) = func.body_mut() {
                optimizer.process(body);
            }
        }
    }
}

struct VacuumOptimizer<'a> {
    arena: &'a mut IdArena<ExpressionMarker, Expression>,
}

impl<'a> VacuumOptimizer<'a> {
    fn process(&mut self, expr_id: &mut ExpressionId) {
        let block_type = self.arena[*expr_id].type_;
        let expr_clone = self.arena[*expr_id].clone();
        match expr_clone.kind {
            ExpressionKind::Block { name, mut list } => {
                for child in list.iter_mut() {
                    self.process(child);
                }

                let mut new_list = Vec::new();
                let list_len = list.len();
                let mut i = 0;
                while i < list_len {
                    let child_id = list[i];
                    let child_kind = self.arena[child_id].kind.clone();
                    if let ExpressionKind::Block {
                        name: None,
                        list: child_list,
                    } = child_kind
                    {
                        for cc in child_list {
                            self.add_to_block(&mut new_list, cc, block_type, i == list_len - 1);
                        }
                    } else {
                        self.add_to_block(&mut new_list, child_id, block_type, i == list_len - 1);
                    }
                    if self.arena[child_id].type_ == Type::UNREACHABLE {
                        break;
                    }
                    i += 1;
                }

                if name.is_none() {
                    if new_list.is_empty() {
                        self.arena[*expr_id].kind = ExpressionKind::Nop;
                        self.arena[*expr_id].type_ = Type::NONE;
                        return;
                    } else if new_list.len() == 1 {
                        let child_id = new_list[0];
                        *expr_id = child_id;
                        return;
                    }
                }
                self.arena[*expr_id].kind = ExpressionKind::Block {
                    name,
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
                self.process(&mut if_true);
                if let Some(mut f) = if_false {
                    self.process(&mut f);
                    if_false = Some(f);
                }

                if let ExpressionKind::Const(lit) = &self.arena[condition].kind {
                    let is_true = match lit {
                        Literal::I32(v) => *v != 0,
                        Literal::I64(v) => *v != 0,
                        _ => false,
                    };
                    if is_true {
                        *expr_id = if_true;
                    } else if let Some(false_branch) = if_false {
                        *expr_id = false_branch;
                    } else {
                        self.arena[*expr_id].kind = ExpressionKind::Nop;
                        self.arena[*expr_id].type_ = Type::NONE;
                    }
                    return;
                }

                if let Some(f) = if_false {
                    if matches!(self.arena[f].kind, ExpressionKind::Nop) {
                        if_false = None;
                    }
                }

                if matches!(self.arena[if_true].kind, ExpressionKind::Nop) {
                    if let Some(f) = if_false.take() {
                        if_true = f;
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
            ExpressionKind::Drop { mut value } => {
                self.process(&mut value);
                if !EffectAnalyzer::analyze(value, self.arena).has_side_effects() {
                    self.arena[*expr_id].kind = ExpressionKind::Nop;
                    self.arena[*expr_id].type_ = Type::NONE;
                } else {
                    self.arena[*expr_id].kind = ExpressionKind::Drop { value };
                }
            }
            ExpressionKind::Loop { name, mut body } => {
                self.process(&mut body);
                self.arena[*expr_id].kind = ExpressionKind::Loop { name, body };
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
            _ => {}
        }
        self.arena[*expr_id].kind = expr_kind;
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
}
