use crate::collections::id_arena::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::module::Module;
use crate::ir::visitor::Visitor;
use crate::passes::Pass;
use std::collections::HashMap;

/// Minifies names in the module (functions, globals, etc.)
pub struct MinifyNames;

impl Pass for MinifyNames {
    fn name(&self) -> &str {
        "MinifyNames"
    }

    fn run(&mut self, module: &mut Module) {
        let mut _name_map = HashMap::new();
        let mut next_id = 0;

        let get_next_name = |next_id: &mut usize| {
            let mut name = String::new();
            let mut id = *next_id;
            loop {
                let char = ((id % 26) as u8 + b'a') as char;
                name.insert(0, char);
                id /= 26;
                if id == 0 {
                    break;
                }
            }
            *next_id += 1;
            name
        };

        // 1. Minify function names
        for (_, func) in &mut module.functions {
            let old_name = func.name.clone();
            let new_name = get_next_name(&mut next_id);
            _name_map.insert(old_name, new_name.clone());
            func.name = new_name;
        }

        // 2. Minify global names
        for (_, global) in &mut module.globals {
            let old_name = global.name.clone();
            let new_name = get_next_name(&mut next_id);
            _name_map.insert(old_name, new_name.clone());
            global.name = new_name;
        }

        // 4. Update local branch names
        let (arena, _, functions, _, _, _, _, _, _, _, _, _, _) = module.parts_mut();
        for (_, func) in functions {
            let mut local_next_id = 0;
            let mut local_name_map = HashMap::new();

            if let Some(mut body) = func.body() {
                let mut collector = BranchNameCollector {
                    next_id: &mut local_next_id,
                    name_map: &mut local_name_map,
                };
                collector.visit(&mut body, arena);

                // Update local branch names
                let mut branch_updater = BranchNameUpdater {
                    name_map: &local_name_map,
                };
                branch_updater.visit(&mut body, arena);
                func.set_body(body);
            }

            // Minify local variable names
            for (_, local) in func.locals_mut().iter_mut() {
                if !local.name.is_empty() {
                    local.name = get_next_name(&mut local_next_id);
                }
            }
        }
    }
}

struct BranchNameCollector<'map> {
    next_id: &'map mut usize,
    name_map: &'map mut HashMap<String, String>,
}

impl<'map> Visitor for BranchNameCollector<'map> {
    fn visit(
        &mut self,
        expr_id: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        match &mut arena[*expr_id].kind {
            ExpressionKind::Block { name, .. } | ExpressionKind::Loop { name, .. } => {
                if let Some(old_name) = name {
                    if !self.name_map.contains_key(old_name) {
                        let mut gen_name = String::new();
                        let mut id = *self.next_id;
                        loop {
                            let char = ((id % 26) as u8 + b'a') as char;
                            gen_name.insert(0, char);
                            id /= 26;
                            if id == 0 {
                                break;
                            }
                        }
                        *self.next_id += 1;
                        self.name_map.insert(old_name.clone(), gen_name);
                    }
                }
            }
            _ => {}
        }
        self.visit_children(expr_id, arena);
    }
}

struct BranchNameUpdater<'map> {
    name_map: &'map HashMap<String, String>,
}

impl<'map> Visitor for BranchNameUpdater<'map> {
    fn visit(
        &mut self,
        expr_id: &mut ExpressionId,
        arena: &mut IdArena<ExpressionMarker, Expression>,
    ) {
        match &mut arena[*expr_id].kind {
            ExpressionKind::Block { name, .. } | ExpressionKind::Loop { name, .. } => {
                if let Some(old_name) = name {
                    if let Some(new_name) = self.name_map.get(old_name) {
                        *name = Some(new_name.clone());
                    }
                }
            }
            ExpressionKind::Break { name, .. } => {
                if let Some(new_name) = self.name_map.get(name) {
                    *name = new_name.clone();
                }
            }
            ExpressionKind::Switch { names, default, .. } => {
                for target in names.iter_mut() {
                    if let Some(new_name) = self.name_map.get(target.as_str()) {
                        *target = new_name.clone();
                    }
                }
                if let Some(new_name) = self.name_map.get(default.as_str()) {
                    *default = new_name.clone();
                }
            }
            _ => {}
        }
        self.visit_children(expr_id, arena);
    }
}

/// Removes all names from the module where possible.
pub struct StripNames;

impl Pass for StripNames {
    fn name(&self) -> &str {
        "StripNames"
    }

    fn run(&mut self, module: &mut Module) {
        // Clear all debug names
        for (_, func) in &mut module.functions {
            func.name = String::new();
            for (_, local) in func.locals_mut().iter_mut() {
                local.name.clear();
            }
        }
        for (_, global) in &mut module.globals {
            global.name = String::new();
        }
        module.annotations.clear();
    }
}

/// Minifies external export names.
pub struct MinifyExports;

impl Pass for MinifyExports {
    fn name(&self) -> &str {
        "MinifyExports"
    }

    fn run(&mut self, module: &mut Module) {
        for (next_id, export) in module.exports.iter_mut().enumerate() {
            let mut name = String::new();
            let mut id = next_id;
            loop {
                let char = ((id % 26) as u8 + b'a') as char;
                name.insert(0, char);
                id /= 26;
                if id == 0 {
                    break;
                }
            }
            export.name = name;
        }
    }
}

/// Minifies external import module/field names.
pub struct MinifyImports;

impl Pass for MinifyImports {
    fn name(&self) -> &str {
        "MinifyImports"
    }

    fn run(&mut self, _module: &mut Module) {
        // TODO: Re-implement using new import architecture
    }
}
