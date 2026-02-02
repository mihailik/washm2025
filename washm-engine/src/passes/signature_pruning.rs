use crate::ir::expression::{ExprRef, ExpressionKind};
use crate::ir::module::Module;
use crate::ir::visitor::Visitor;
use crate::ir::{Literal, Type};
use crate::passes::Pass;
use std::collections::{HashMap, HashSet};

pub struct SignaturePruning;

impl Pass for SignaturePruning {
    fn name(&self) -> &str {
        "SignaturePruning"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        // 1. Collect usage info for all functions
        let mut pruning_info = HashMap::new();

        let mut exported_ids = HashSet::new();
        for export in &module.exports {
            if let crate::ir::module::ExportKind::Function(id) = export.kind {
                exported_ids.insert(id);
            }
        }

        let mut address_taken_ids = HashSet::new();
        for (_, elem) in &module.elements {
            for &func_id in &elem.func_indices {
                address_taken_ids.insert(func_id);
            }
        }

        let mut candidates = HashSet::new();
        for (id, func) in &module.functions {
            if !exported_ids.contains(&id) && !address_taken_ids.contains(&id) {
                if let Some(params) = get_param_types(func.params) {
                    candidates.insert(id);
                    pruning_info.insert(id, vec![ParamStatus::Unused; params.len()]);
                }
            }
        }

        if candidates.is_empty() {
            return;
        }

        // 2. Scan all function bodies to check for param usage (local.get)
        for (id, func) in &mut module.functions {
            let param_ids = func.param_ids.clone();
            if let Some(body) = func.body_mut() {
                let mut scanner = ParamUsageScanner {
                    usage: if candidates.contains(&id) {
                        pruning_info.get_mut(&id)
                    } else {
                        None
                    },
                    param_ids: &param_ids,
                };
                scanner.visit_expression(body);
            }
        }

        // 3. Scan all call sites
        for (_, func) in &mut module.functions {
            if let Some(body) = func.body_mut() {
                let mut call_scanner = CallSiteScanner {
                    candidates: &candidates,
                    pruning_info: &mut pruning_info,
                };
                call_scanner.visit_expression(body);
            }
        }

        // 4. Apply pruning
        let mut pruned_functions = HashSet::new();

        for (id, func) in &mut module.functions {
            if let Some(infos) = pruning_info.get(&id) {
                let old_params_vec = get_param_types(func.params).unwrap_or_default();
                if old_params_vec.len() != infos.len() {
                    continue;
                }

                let mut new_params = Vec::new();
                let mut new_param_ids = Vec::new();
                let mut constant_replacements = HashMap::new();

                let param_ids = func.param_ids.clone();

                for (i, status) in infos.iter().enumerate() {
                    match status {
                        ParamStatus::Read | ParamStatus::Written | ParamStatus::Variable => {
                            new_params.push(old_params_vec[i]);
                            new_param_ids.push(param_ids[i]);
                        }
                        ParamStatus::Unused => {
                            // Deallocate the formal parameter local, it's totally unused
                            func.locals_mut().dealloc(param_ids[i].cast());
                        }
                        ParamStatus::Constant(lit) => {
                            constant_replacements.insert(param_ids[i], *lit);
                            // Also deallocate since it's now a constant
                            func.locals_mut().dealloc(param_ids[i].cast());
                        }
                    }
                }

                if new_param_ids.len() == func.param_ids.len() {
                    continue;
                }

                if let Some(new_ty) = make_param_type(&new_params) {
                    func.params = new_ty;
                    func.param_ids = new_param_ids;
                    pruned_functions.insert(func.name.clone());

                    if let Some(body) = func.body_mut() {
                        let mut remapper = LocalRemapper {
                            const_replacements: constant_replacements,
                        };
                        remapper.visit(body);
                    }
                }
            }
        }

        // 5. Update Call Sites
        if !pruned_functions.is_empty() {
            for (_, func) in &mut module.functions {
                if let Some(body) = func.body_mut() {
                    let mut call_updater = CallUpdater {
                        pruning_info: &pruning_info,
                    };
                    call_updater.visit(body);
                }
            }
        }
    }
}

// Helper to interpret Type as list of params
fn get_param_types(ty: Type) -> Option<Vec<Type>> {
    if ty == Type::NONE {
        Some(vec![])
    } else if ty.is_basic() {
        Some(vec![ty])
    } else {
        None
    }
}

fn make_param_type(types: &[Type]) -> Option<Type> {
    if types.is_empty() {
        Some(Type::NONE)
    } else if types.len() == 1 {
        Some(types[0])
    } else {
        None
    }
}

#[derive(Clone, Debug, PartialEq)]
enum ParamStatus {
    Unused,
    Read,
    Written,
    Constant(Literal),
    Variable,
}

#[allow(dead_code)]
struct ParamUsageScanner<'a> {
    usage: Option<&'a mut Vec<ParamStatus>>,
    param_ids: &'a Vec<crate::ir::LocalId>,
}

impl<'a, 'b> Visitor<'b> for ParamUsageScanner<'a> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'b>) {
        if let Some(usage) = &mut self.usage {
            match &mut expr.kind {
                ExpressionKind::LocalGet { id } => {
                    if let Some(pos) = self.param_ids.iter().position(|&p| p == *id) {
                        if usage[pos] == ParamStatus::Unused {
                            usage[pos] = ParamStatus::Read;
                        }
                    }
                }
                ExpressionKind::LocalSet { id, .. } | ExpressionKind::LocalTee { id, .. } => {
                    if let Some(pos) = self.param_ids.iter().position(|&p| p == *id) {
                        usage[pos] = ParamStatus::Written;
                    }
                }
                _ => {}
            }
        }
        self.visit_children(expr);
    }
}

struct CallSiteScanner<'a> {
    candidates: &'a HashSet<crate::ir::FuncId>,
    pruning_info: &'a mut HashMap<crate::ir::FuncId, Vec<ParamStatus>>,
}

impl<'a, 'b> Visitor<'b> for CallSiteScanner<'a> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'b>) {
        if let ExpressionKind::Call {
            target, operands, ..
        } = &expr.kind
        {
            if self.candidates.contains(target) {
                if let Some(statuses) = self.pruning_info.get_mut(target) {
                    for (i, op) in operands.iter().enumerate() {
                        if i >= statuses.len() {
                            break;
                        }

                        // Check if op is constant
                        let _is_const = matches!(op.kind, ExpressionKind::Const(_));
                        let const_val = if let ExpressionKind::Const(lit) = &op.kind {
                            Some(*lit)
                        } else {
                            None
                        };

                        let status = &mut statuses[i];

                        match status {
                            ParamStatus::Unused => {}
                            ParamStatus::Written => {
                                *status = ParamStatus::Variable;
                            }
                            ParamStatus::Read => {
                                if let Some(lit) = const_val {
                                    *status = ParamStatus::Constant(lit);
                                } else {
                                    *status = ParamStatus::Variable;
                                }
                            }
                            ParamStatus::Constant(current_lit) => {
                                if let Some(lit) = const_val {
                                    if lit != *current_lit {
                                        *status = ParamStatus::Variable;
                                    }
                                } else {
                                    *status = ParamStatus::Variable;
                                }
                            }
                            ParamStatus::Variable => {}
                        }
                    }
                }
            }
        }
        self.visit_children(expr);
    }
}

struct LocalRemapper {
    const_replacements: HashMap<crate::ir::LocalId, Literal>,
}

impl<'b> Visitor<'b> for LocalRemapper {
    fn visit_expression(&mut self, expr: &mut ExprRef<'b>) {
        self.visit_children(expr);

        if let ExpressionKind::LocalGet { id } = &mut expr.kind {
            if let Some(lit) = self.const_replacements.get(id) {
                expr.kind = ExpressionKind::Const(*lit);
            }
        }
    }
}

struct CallUpdater<'a> {
    pruning_info: &'a HashMap<crate::ir::FuncId, Vec<ParamStatus>>,
}

impl<'a, 'b> Visitor<'b> for CallUpdater<'a> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'b>) {
        self.visit_children(expr);

        if let ExpressionKind::Call {
            target, operands, ..
        } = &mut expr.kind
        {
            if let Some(statuses) = self.pruning_info.get(target) {
                let mut new_operands = bumpalo::collections::Vec::new_in(operands.bump());

                for (i, op) in operands.iter().enumerate() {
                    if i < statuses.len() {
                        let status = &statuses[i];
                        if matches!(
                            status,
                            ParamStatus::Read | ParamStatus::Written | ParamStatus::Variable
                        ) {
                            new_operands.push(*op);
                        }
                    } else {
                        new_operands.push(*op);
                    }
                }
                *operands = new_operands;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::{Expression, ExpressionKind, IrBuilder};
    use crate::ir::module::{Function, Module};
    use crate::ir::{Literal, Type};
    use bumpalo::Bump;

    #[test]
    fn test_prune_unused_params() {
        let bump = Bump::new();
        let mut module = Module::new(&bump);

        let nop = Expression::nop(&bump);
        let func = Function::new(
            "unused_params".to_string(),
            Type::I32,
            Type::NONE,
            vec![Type::I32, Type::F32],
            Some(nop),
        );
        let callee_id = module.add_function(func);

        let builder = IrBuilder::new(&bump);
        let arg1 = builder.const_(Literal::I32(1));
        let args = bumpalo::collections::Vec::from_iter_in([arg1], &bump);
        let call = builder.call(callee_id, args, Type::NONE, false);

        let caller = Function::new(
            "caller".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(call),
        );
        let caller_id = module.add_function(caller);
        module.export_function(caller_id, "main".to_string());

        let mut pass = SignaturePruning;
        pass.run(&mut module);

        let callee = &module.functions[callee_id];
        assert_eq!(callee.params, Type::NONE);

        let caller = &module.functions[caller_id];
        if let Some(body) = &caller.body() {
            if let ExpressionKind::Call { operands, .. } = &body.kind {
                assert_eq!(operands.len(), 0);
            } else {
                panic!("Expected call");
            }
        }
    }

    #[test]
    fn test_constant_propagation_params() {
        let bump = Bump::new();
        let mut module = Module::new(&bump);

        let builder = IrBuilder::new(&bump);
        let get0 = builder.local_get(0, Type::I32);

        let func = Function::new(
            "const_param".to_string(),
            Type::I32,
            Type::I32,
            vec![Type::I32, Type::I32],
            Some(get0),
        );
        let callee_id = module.add_function(func);

        let c1_arg1 = builder.const_(Literal::I32(42));
        let c1_args = bumpalo::collections::Vec::from_iter_in([c1_arg1], &bump);
        let call1 = builder.call(callee_id, c1_args, Type::I32, false);
        let caller1 = Function::new(
            "caller1".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(call1),
        );
        let caller1_id = module.add_function(caller1);

        let c2_arg1 = builder.const_(Literal::I32(42));
        let c2_args = bumpalo::collections::Vec::from_iter_in([c2_arg1], &bump);
        let call2 = builder.call(callee_id, c2_args, Type::I32, false);
        let caller2 = Function::new(
            "caller2".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(call2),
        );
        let caller2_id = module.add_function(caller2);

        module.export_function(caller1_id, "c1".to_string());
        module.export_function(caller2_id, "c2".to_string());

        let mut pass = SignaturePruning;
        pass.run(&mut module);

        let callee = &module.functions[callee_id];
        assert_eq!(callee.params, Type::NONE);

        if let Some(body) = &callee.body() {
            if let ExpressionKind::Const(lit) = &body.kind {
                assert_eq!(
                    match lit {
                        Literal::I32(v) => *v,
                        _ => panic!("Expected I32"),
                    },
                    42
                );
            } else {
                panic!("Body should be const");
            }
        }
    }
}
