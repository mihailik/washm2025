use crate::ir::expression::{ExprRef, ExpressionKind};
use crate::ir::module::Module;
use crate::passes::Pass;
use crate::ir::visitor::Visitor;
use crate::ir::{Literal, Type};
use std::collections::{HashMap, HashSet};

pub struct SignaturePruning;

impl Pass for SignaturePruning {
    fn name(&self) -> &str {
        "SignaturePruning"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        // 1. Collect usage info for all functions
        let mut pruning_info = HashMap::new();

        let mut exported_names = HashSet::new();
        let func_import_count = module
            .imports
            .iter()
            .filter(|i| matches!(i.kind, crate::ir::module::ImportKind::Function(_, _)))
            .count();

        for export in &module.exports {
            if let crate::ir::module::ExportKind::Function = export.kind {
                if export.index >= func_import_count as u32 {
                    let defined_idx = (export.index as usize) - func_import_count;
                    if let Some(f) = module.functions.get(defined_idx) {
                        exported_names.insert(f.name.clone());
                    }
                }
            }
        }

        let mut address_taken_names = HashSet::new();
        for elem in &module.elements {
            for &func_idx in &elem.func_indices {
                if func_idx >= func_import_count as u32 {
                    let defined_idx = (func_idx as usize) - func_import_count;
                    if let Some(f) = module.functions.get(defined_idx) {
                        address_taken_names.insert(f.name.clone());
                    }
                }
            }
        }

        let mut candidates = HashSet::new();
        for func in &module.functions {
            if !exported_names.contains(&func.name) && !address_taken_names.contains(&func.name) {
                if let Some(params) = get_param_types(func.params) {
                    candidates.insert(func.name.clone());
                    pruning_info.insert(func.name.clone(), vec![ParamStatus::Unused; params.len()]);
                }
            }
        }

        if candidates.is_empty() {
            return;
        }

        // 2. Scan all function bodies to check for param usage (local.get)
        for func in &module.functions {
            if let Some(body) = &func.body {
                let num_params = if let Some(p) = get_param_types(func.params) {
                    p.len()
                } else {
                    0
                };
                let mut scanner = ParamUsageScanner {
                    target_func: &func.name,
                    usage: if candidates.contains(&func.name) {
                        pruning_info.get_mut(&func.name)
                    } else {
                        None
                    },
                    num_params,
                };
                let mut body_ref = *body;
                scanner.visit_expression(&mut body_ref);
            }
        }

        // 3. Scan all call sites
        for func in &module.functions {
            if let Some(body) = &func.body {
                let mut call_scanner = CallSiteScanner {
                    candidates: &candidates,
                    pruning_info: &mut pruning_info,
                };
                let mut body_ref = *body;
                call_scanner.visit_expression(&mut body_ref);
            }
        }

        // 4. Apply pruning
        let mut pruned_functions = HashSet::new();

        for func in &mut module.functions {
            if let Some(infos) = pruning_info.get(&func.name) {
                let old_params_vec = get_param_types(func.params).unwrap_or_default();
                if old_params_vec.len() != infos.len() {
                    continue;
                }

                let mut new_params = Vec::new();
                let mut remap = HashMap::new();
                let mut constant_replacements = HashMap::new();

                let mut _keep_any = false;

                for (i, status) in infos.iter().enumerate() {
                    match status {
                        ParamStatus::Read | ParamStatus::Written | ParamStatus::Variable => {
                            remap.insert(i as u32, new_params.len() as u32);
                            new_params.push(old_params_vec[i]);
                            _keep_any = true;
                        }
                        ParamStatus::Unused => {
                            // Drop
                        }
                        ParamStatus::Constant(lit) => {
                            constant_replacements.insert(i as u32, lit.clone());
                        }
                    }
                }

                if new_params.len() == old_params_vec.len() {
                    continue;
                }

                if let Some(new_ty) = make_param_type(&new_params) {
                    func.params = new_ty;
                    pruned_functions.insert(func.name.clone());

                    if let Some(body) = &mut func.body {
                        let mut remapper = LocalRemapper {
                            param_remap: &remap,
                            const_replacements: &constant_replacements,
                            old_param_count: infos.len() as u32,
                            new_param_count: new_params.len() as u32,
                        };
                        remapper.visit(body);
                    }
                }
            }
        }

        // 5. Update Call Sites
        if !pruned_functions.is_empty() {
            for func in &mut module.functions {
                if let Some(body) = &mut func.body {
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
    target_func: &'a str,
    usage: Option<&'a mut Vec<ParamStatus>>,
    num_params: usize,
}

impl<'a, 'b> Visitor<'b> for ParamUsageScanner<'a> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'b>) {
        if let Some(usage) = &mut self.usage {
            match expr.kind {
                ExpressionKind::LocalGet { index } => {
                    if (index as usize) < self.num_params
                        && usage[index as usize] == ParamStatus::Unused
                    {
                        usage[index as usize] = ParamStatus::Read;
                    }
                }
                ExpressionKind::LocalSet { index, .. } | ExpressionKind::LocalTee { index, .. } => {
                    if (index as usize) < self.num_params {
                        usage[index as usize] = ParamStatus::Written;
                    }
                }
                _ => {}
            }
        }
        self.visit_children(expr);
    }
}

struct CallSiteScanner<'a> {
    candidates: &'a HashSet<String>,
    pruning_info: &'a mut HashMap<String, Vec<ParamStatus>>,
}

impl<'a, 'b> Visitor<'b> for CallSiteScanner<'a> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'b>) {
        if let ExpressionKind::Call {
            target, operands, ..
        } = &expr.kind
        {
            if self.candidates.contains(*target) {
                if let Some(statuses) = self.pruning_info.get_mut(*target) {
                    for (i, op) in operands.iter().enumerate() {
                        if i >= statuses.len() {
                            break;
                        }

                        // Check if op is constant
                        let _is_const = matches!(op.kind, ExpressionKind::Const(_));
                        let const_val = if let ExpressionKind::Const(lit) = &op.kind {
                            Some(lit.clone())
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

struct LocalRemapper<'a> {
    param_remap: &'a HashMap<u32, u32>,
    const_replacements: &'a HashMap<u32, Literal>,
    old_param_count: u32,
    new_param_count: u32,
    // bump removed
}

impl<'a, 'b> Visitor<'b> for LocalRemapper<'a> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'b>) {
        self.visit_children(expr);

        match &mut expr.kind {
            ExpressionKind::LocalGet { index }
            | ExpressionKind::LocalSet { index, .. }
            | ExpressionKind::LocalTee { index, .. } => {
                if *index < self.old_param_count {
                    if let Some(lit) = self.const_replacements.get(index) {
                        if matches!(expr.kind, ExpressionKind::LocalGet { .. }) {
                            expr.kind = ExpressionKind::Const(lit.clone());
                        }
                    } else if let Some(new_idx) = self.param_remap.get(index) {
                        *index = *new_idx;
                    }
                } else {
                    let local_var_idx = *index - self.old_param_count;
                    *index = self.new_param_count + local_var_idx;
                }
            }
            _ => {}
        }
    }
}

struct CallUpdater<'a> {
    pruning_info: &'a HashMap<String, Vec<ParamStatus>>,
}

impl<'a, 'b> Visitor<'b> for CallUpdater<'a> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'b>) {
        self.visit_children(expr);

        if let ExpressionKind::Call {
            target, operands, ..
        } = &mut expr.kind
        {
            if let Some(statuses) = self.pruning_info.get(*target) {
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
        module.add_function(func);

        let builder = IrBuilder::new(&bump);
        let arg1 = builder.const_(Literal::I32(1));
        let args = bumpalo::collections::Vec::from_iter_in([arg1], &bump);
        let call = builder.call("unused_params", args, Type::NONE, false);

        let caller = Function::new(
            "caller".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(call),
        );
        module.add_function(caller);
        module.export_function(1, "main".to_string());

        let mut pass = SignaturePruning;
        pass.run(&mut module);

        let callee = &module.functions[0];
        assert_eq!(callee.params, Type::NONE);

        let caller = &module.functions[1];
        if let Some(body) = &caller.body {
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
        module.add_function(func);

        let c1_arg1 = builder.const_(Literal::I32(42));
        let c1_args = bumpalo::collections::Vec::from_iter_in([c1_arg1], &bump);
        let call1 = builder.call("const_param", c1_args, Type::I32, false);
        let caller1 = Function::new(
            "caller1".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(call1),
        );
        module.add_function(caller1);

        let c2_arg1 = builder.const_(Literal::I32(42));
        let c2_args = bumpalo::collections::Vec::from_iter_in([c2_arg1], &bump);
        let call2 = builder.call("const_param", c2_args, Type::I32, false);
        let caller2 = Function::new(
            "caller2".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(call2),
        );
        module.add_function(caller2);

        module.export_function(1, "c1".to_string());
        module.export_function(2, "c2".to_string());

        let mut pass = SignaturePruning;
        pass.run(&mut module);

        let callee = &module.functions[0];
        assert_eq!(callee.params, Type::NONE);

        if let Some(body) = &callee.body {
            if let ExpressionKind::Const(lit) = &body.kind {
                assert_eq!(*lit, Literal::I32(42));
            } else {
                panic!("Body should be const");
            }
        }
    }
}
