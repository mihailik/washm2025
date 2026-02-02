use crate::ir::expression::{ExprRef, ExpressionKind};
use crate::ir::module::Module;
use crate::passes::Pass;
use std::collections::HashSet;

/// Removes names from blocks and loops that are not targeted by any branch.
pub struct RemoveUnusedNames;

impl Pass for RemoveUnusedNames {
    fn name(&self) -> &str {
        "RemoveUnusedNames"
    }

    fn run(&mut self, module: &mut Module) {
        let mut optimizer = NameOptimizer;
        for (_, func) in &mut module.functions {
            if let Some(mut body) = func.body() {
                let mut used_names = HashSet::new();
                optimizer.process(&mut body, &mut used_names);
                *func.body_mut() = Some(body);
            }
        }
    }
}

struct NameOptimizer;

impl NameOptimizer {
    fn process<'a>(&mut self, expr: &mut ExprRef<'a>, used_names: &mut HashSet<&'a str>) {
        match &mut expr.kind {
            ExpressionKind::Block { name, list } => {
                for child in list.iter_mut() {
                    self.process(child, used_names);
                }
                if let Some(n) = name {
                    if !used_names.contains(n) {
                        *name = None;
                    } else {
                        used_names.remove(n);
                    }
                }
            }
            ExpressionKind::Loop { name, body } => {
                self.process(body, used_names);
                if let Some(n) = name {
                    if !used_names.contains(n) {
                        *name = None;
                    } else {
                        used_names.remove(n);
                    }
                }
            }
            ExpressionKind::If {
                condition,
                if_true,
                if_false,
            } => {
                self.process(condition, used_names);
                self.process(if_true, used_names);
                if let Some(f) = if_false {
                    self.process(f, used_names);
                }
            }
            ExpressionKind::Break {
                name,
                condition,
                value,
            } => {
                used_names.insert(name);
                if let Some(c) = condition {
                    self.process(c, used_names);
                }
                if let Some(v) = value {
                    self.process(v, used_names);
                }
            }
            ExpressionKind::Switch {
                names,
                default,
                condition,
                value,
            } => {
                for &n in names.iter() {
                    used_names.insert(n);
                }
                used_names.insert(default);
                self.process(condition, used_names);
                if let Some(v) = value {
                    self.process(v, used_names);
                }
            }
            ExpressionKind::Call { operands, .. }
            | ExpressionKind::CallIndirect { operands, .. } => {
                for op in operands.iter_mut() {
                    self.process(op, used_names);
                }
            }
            ExpressionKind::LocalSet { value, .. }
            | ExpressionKind::LocalTee { value, .. }
            | ExpressionKind::GlobalSet { value, .. }
            | ExpressionKind::Unary { value, .. }
            | ExpressionKind::Drop { value }
            | ExpressionKind::Load { ptr: value, .. }
            | ExpressionKind::MemoryGrow { delta: value, .. } => {
                self.process(value, used_names);
            }
            ExpressionKind::Binary { left, right, .. }
            | ExpressionKind::Store {
                ptr: left,
                value: right,
                ..
            } => {
                self.process(left, used_names);
                self.process(right, used_names);
            }
            ExpressionKind::Select {
                if_true,
                if_false,
                condition,
                ..
            } => {
                self.process(if_true, used_names);
                self.process(if_false, used_names);
                self.process(condition, used_names);
            }
            ExpressionKind::Return { value } => {
                if let Some(v) = value {
                    self.process(v, used_names);
                }
            }
            ExpressionKind::MemoryInit {
                dest, offset, size, ..
            } => {
                self.process(dest, used_names);
                self.process(offset, used_names);
                self.process(size, used_names);
            }
            ExpressionKind::DataDrop { .. }
            | ExpressionKind::MemoryCopy { .. }
            | ExpressionKind::MemoryFill { .. } => {}
            ExpressionKind::Const(_)
            | ExpressionKind::LocalGet { .. }
            | ExpressionKind::GlobalGet { .. }
            | ExpressionKind::Nop
            | ExpressionKind::Unreachable
            | ExpressionKind::MemorySize { .. } => {}
            _ => {}
        }
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
    fn test_remove_unused_name() {
        let bump = Bump::new();
        let mut list = BumpVec::new_in(&bump);
        list.push(ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Nop,
            type_: Type::NONE,
        })));

        let block_expr = bump.alloc(Expression {
            kind: ExpressionKind::Block {
                name: Some("unused"),
                list,
            },
            type_: Type::NONE,
        });

        let mut module = Module::new(&bump);
        module.add_function(Function::new(
            "test".into(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(ExprRef::new(block_expr)),
        ));

        let mut pass = RemoveUnusedNames;
        pass.run(&mut module);

        let binding = module.functions[module.functions.iter().next().unwrap().0].body();
        let body = binding.as_ref().unwrap();
        if let ExpressionKind::Block { name, .. } = &body.kind {
            assert!(name.is_none());
        } else {
            panic!("Expected block");
        }
    }

    #[test]
    fn test_keep_used_name() {
        let bump = Bump::new();
        let mut list = BumpVec::new_in(&bump);

        list.push(ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Break {
                name: "used",
                condition: None,
                value: None,
            },
            type_: Type::UNREACHABLE,
        })));

        let block_expr = bump.alloc(Expression {
            kind: ExpressionKind::Block {
                name: Some("used"),
                list,
            },
            type_: Type::NONE,
        });

        let mut module = Module::new(&bump);
        module.add_function(Function::new(
            "test".into(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(ExprRef::new(block_expr)),
        ));

        let mut pass = RemoveUnusedNames;
        pass.run(&mut module);

        let binding = module.functions[module.functions.iter().next().unwrap().0].body();
        let body = binding.as_ref().unwrap();
        if let ExpressionKind::Block { name, .. } = &body.kind {
            assert_eq!(name.unwrap(), "used");
        } else {
            panic!("Expected block");
        }
    }
}
