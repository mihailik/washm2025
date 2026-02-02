use crate::ir::expression::{ExprRef, ExpressionKind};
use crate::ir::visitor::Visitor;
use crate::ir::Type;

pub struct ReFinalize;

impl ReFinalize {
    pub fn run(mut expr: ExprRef) {
        let mut visitor = ReFinalizeVisitor;
        visitor.visit(&mut expr);
    }
}

struct ReFinalizeVisitor;

impl<'a> Visitor<'a> for ReFinalizeVisitor {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        self.visit_children(expr);

        // Now update type based on children
        match &expr.kind {
            ExpressionKind::Block { list, .. } => {
                if list.is_empty() {
                    unsafe {
                        (*expr.as_ptr()).type_ = Type::NONE;
                    }
                } else {
                    let last = list.last().unwrap();
                    // TODO: handle breaks to this block
                    let ty = last.type_;
                    if ty != Type::UNREACHABLE {
                        unsafe {
                            (*expr.as_ptr()).type_ = ty;
                        }
                    }
                    // If it's unreachable, it stays reachable unless all paths are unreachable or breaks are handled?
                    // Simplified: just take last type for now if concrete.

                    // Correct logic:
                    // Stop at first Unreachable.
                    // If no Unreachable, type is type of last element.
                    // BUT breaks can return values.
                    // Binaryen's ReFinalize loops over breaks targeting this block.
                    // For now, Asyncify mostly cares about Block as a container.

                    // Let's assume standard behavior: type of block is type of last expression,
                    // unless earlier expression is Unreachable.
                    let mut saw_unreachable = false;
                    for child in list.iter() {
                        if child.type_ == Type::UNREACHABLE {
                            saw_unreachable = true;
                            break;
                        }
                    }
                    if saw_unreachable {
                        unsafe {
                            (*expr.as_ptr()).type_ = Type::UNREACHABLE;
                        }
                    } else {
                        unsafe {
                            (*expr.as_ptr()).type_ = list.last().unwrap().type_;
                        }
                    }
                }
            }
            ExpressionKind::If {
                if_true, if_false, ..
            } => {
                let true_type = if_true.type_;
                if let Some(false_branch) = if_false {
                    let false_type = false_branch.type_;
                    unsafe {
                        (*expr.as_ptr()).type_ = Type::get_lub(true_type, false_type);
                    }
                } else {
                    unsafe {
                        (*expr.as_ptr()).type_ = Type::NONE;
                    }
                }
            }
            ExpressionKind::Loop { body, .. } => unsafe {
                (*expr.as_ptr()).type_ = body.type_;
            },
            ExpressionKind::Break { .. } => unsafe {
                (*expr.as_ptr()).type_ = Type::UNREACHABLE;
            },
            ExpressionKind::Return { .. } => unsafe {
                (*expr.as_ptr()).type_ = Type::UNREACHABLE;
            },
            ExpressionKind::Unreachable => unsafe {
                (*expr.as_ptr()).type_ = Type::UNREACHABLE;
            },
            ExpressionKind::Call { .. } => {
                // Call type depends on signature, which doesn't change usually.
                // But if we change it to Unreachable...
                if expr.type_ != Type::UNREACHABLE {
                    // keep existing
                }
            }
            // ... other generic expressions
            _ => {}
        }
    }
}
