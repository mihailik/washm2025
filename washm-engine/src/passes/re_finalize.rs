use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::visitor::Visitor;
use crate::ir::Type;

pub struct ReFinalize;

impl ReFinalize {
    pub fn run(expr_id: &mut ExpressionId, arena: &mut IdArena<ExpressionMarker, Expression>) {
        let mut visitor = ReFinalizeVisitor;
        visitor.visit(expr_id, arena);
    }
}

struct ReFinalizeVisitor;

impl Visitor for ReFinalizeVisitor {
    fn visit(
        &mut self,
        expr_id: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        self.visit_children(expr_id, arena);

        // Now update type based on children
        let kind = arena[*expr_id].kind.clone();
        match kind {
            ExpressionKind::Block { list, .. } => {
                if list.is_empty() {
                    arena[*expr_id].type_ = Type::NONE;
                } else {
                    let mut saw_unreachable = false;
                    for &child in &list {
                        if arena[child].type_ == Type::UNREACHABLE {
                            saw_unreachable = true;
                            break;
                        }
                    }
                    if saw_unreachable {
                        arena[*expr_id].type_ = Type::UNREACHABLE;
                    } else {
                        arena[*expr_id].type_ = arena[*list.last().unwrap()].type_;
                    }
                }
            }
            ExpressionKind::If {
                if_true, if_false, ..
            } => {
                let true_type = arena[if_true].type_;
                if let Some(false_branch) = if_false {
                    let false_type = arena[false_branch].type_;
                    arena[*expr_id].type_ = Type::get_lub(true_type, false_type);
                } else {
                    arena[*expr_id].type_ = Type::NONE;
                }
            }
            ExpressionKind::Loop { body, .. } => {
                arena[*expr_id].type_ = arena[body].type_;
            }
            ExpressionKind::Break { .. } => {
                arena[*expr_id].type_ = Type::UNREACHABLE;
            }
            ExpressionKind::Return { .. } => {
                arena[*expr_id].type_ = Type::UNREACHABLE;
            }
            ExpressionKind::Unreachable => {
                arena[*expr_id].type_ = Type::UNREACHABLE;
            }
            _ => {}
        }
    }
}
