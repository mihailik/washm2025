use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::module::Module;
use crate::ir::visitor::Visitor;
use crate::ir::{FuncId, Literal, LocalId, Type};
use crate::passes::Pass;
use std::collections::{HashMap, HashSet};

pub struct SignaturePruning;

impl Pass for SignaturePruning {
    fn name(&self) -> &str {
        "SignaturePruning"
    }

    fn run(&mut self, module: &mut Module) {
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

        for (id, func) in &mut module.functions {
            let param_ids = func.param_ids().to_vec();
            if let Some(mut body) = func.body() {
                let mut scanner = ParamUsageScanner {
                    usage: if candidates.contains(&id) {
                        pruning_info.get_mut(&id)
                    } else {
                        None
                    },
                    param_ids: &param_ids,
                };
                scanner.visit(&mut body, &mut module.expressions);
                func.set_body(body);
            }
        }

        for (_id, func) in &mut module.functions {
            if let Some(mut body) = func.body() {
                let mut call_scanner = CallSiteScanner {
                    candidates: &candidates,
                    pruning_info: &mut pruning_info,
                };
                call_scanner.visit(&mut body, &mut module.expressions);
                func.set_body(body);
            }
        }

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

                let param_ids = func.param_ids().to_vec();

                for (i, status) in infos.iter().enumerate() {
                    match status {
                        ParamStatus::Read | ParamStatus::Written | ParamStatus::Variable => {
                            new_params.push(old_params_vec[i]);
                            new_param_ids.push(param_ids[i]);
                        }
                        ParamStatus::Unused => {
                            func.locals_mut().dealloc(param_ids[i].cast());
                        }
                        ParamStatus::Constant(lit) => {
                            constant_replacements.insert(param_ids[i], *lit);
                            func.locals_mut().dealloc(param_ids[i].cast());
                        }
                    }
                }

                if new_param_ids.len() == func.param_ids().len() {
                    continue;
                }

                if let Some(new_ty) = make_param_type(&new_params) {
                    func.params = new_ty;
                    if let Some(mut_param_ids) = func.param_ids_mut() {
                        *mut_param_ids = new_param_ids;
                    }
                    pruned_functions.insert(func.name.clone());

                    if let Some(mut body) = func.body() {
                        let mut remapper = LocalRemapper {
                            const_replacements: constant_replacements,
                        };
                        remapper.visit(&mut body, &mut module.expressions);
                        func.set_body(body);
                    }
                }
            }
        }

        if !pruned_functions.is_empty() {
            for (_id, func) in &mut module.functions {
                if let Some(mut body) = func.body() {
                    let mut call_updater = CallUpdater {
                        pruning_info: &pruning_info,
                    };
                    call_updater.visit(&mut body, &mut module.expressions);
                    func.set_body(body);
                }
            }
        }
    }
}

fn get_param_types(ty: Type) -> Option<Vec<Type>> {
    Some(ty.tuple_elements())
}

fn make_param_type(types: &[Type]) -> Option<Type> {
    if types.is_empty() {
        Some(Type::NONE)
    } else if types.len() == 1 {
        Some(types[0])
    } else {
        // We probably need a better way to handle tuples if they aren't basic types.
        // For now, if it was already a tuple or basic, this might stay basic.
        // If it was more complex, we might need a type store.
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
    param_ids: &'a Vec<LocalId>,
}

impl<'a> Visitor for ParamUsageScanner<'a> {
    fn visit_expression(
        &mut self,
        expr: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        if let Some(usage) = &mut self.usage {
            match &mut arena[*expr].kind {
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
    }
}

struct CallSiteScanner<'a> {
    candidates: &'a HashSet<FuncId>,
    pruning_info: &'a mut HashMap<FuncId, Vec<ParamStatus>>,
}

impl<'a> Visitor for CallSiteScanner<'a> {
    fn visit_expression(
        &mut self,
        expr: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        let kind = &arena[*expr].kind;
        if let ExpressionKind::Call {
            target, operands, ..
        } = kind
        {
            let target = *target;
            if self.candidates.contains(&target) {
                if let Some(statuses) = self.pruning_info.get_mut(&target) {
                    for (i, &op) in operands.iter().enumerate() {
                        if i >= statuses.len() {
                            break;
                        }

                        let const_val = if let ExpressionKind::Const(lit) = arena[op].kind {
                            Some(lit)
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
    }
}

struct LocalRemapper {
    const_replacements: HashMap<LocalId, Literal>,
}

impl Visitor for LocalRemapper {
    fn visit_expression(
        &mut self,
        expr: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        if let ExpressionKind::LocalGet { id } = &mut arena[*expr].kind {
            if let Some(lit) = self.const_replacements.get(id) {
                arena[*expr].kind = ExpressionKind::Const(*lit);
            }
        }
    }
}

struct CallUpdater<'a> {
    pruning_info: &'a HashMap<FuncId, Vec<ParamStatus>>,
}

impl<'a> Visitor for CallUpdater<'a> {
    fn visit_expression(
        &mut self,
        expr: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        let kind = arena[*expr].kind.clone();
        if let ExpressionKind::Call {
            target, operands, ..
        } = kind
        {
            if let Some(statuses) = self.pruning_info.get(&target) {
                let mut new_operands = Vec::new();

                for (i, &op) in operands.iter().enumerate() {
                    if i < statuses.len() {
                        let status = &statuses[i];
                        if matches!(
                            status,
                            ParamStatus::Read | ParamStatus::Written | ParamStatus::Variable
                        ) {
                            new_operands.push(op);
                        }
                    } else {
                        new_operands.push(op);
                    }
                }

                if new_operands.len() != operands.len() {
                    if let ExpressionKind::Call {
                        operands: old_ops, ..
                    } = &mut arena[*expr].kind
                    {
                        *old_ops = new_operands;
                    }
                }
            }
        }
    }
}
