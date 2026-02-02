use crate::ir::expression::{ExprRef, ExpressionKind};
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

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        let mut name_map = HashMap::new();
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
            name_map.insert(old_name, new_name.clone());
            func.name = new_name;
        }

        // 2. Minify global names
        for (_, global) in &mut module.globals {
            let old_name = global.name.clone();
            let new_name = get_next_name(&mut next_id);
            name_map.insert(old_name, new_name.clone());
            global.name = new_name;
        }

        // 3. Minify table names (if any)
        // Note: Our IR currently has Option<TableLimits> but no explicit name in TableLimits.
        // However, expressions use a table name (string).
        // This suggests there's a disconnect or a single implicit table name.
        // In Binaryen, tables have names.

        // 4. Update local branch names
        for (_, func) in &mut module.functions {
            let mut local_next_id = 0;
            let mut local_name_map = HashMap::new();
            self.minify_local_names(
                func,
                &mut local_next_id,
                &mut local_name_map,
                module.allocator,
            );

            if let Some(mut body) = func.body_mut() {
                // Update local branch names
                let mut branch_updater = BranchNameUpdater {
                    name_map: &local_name_map,
                    allocator: module.allocator,
                };
                branch_updater.visit(&mut body);
            }

            // Minify local variable names
            for name in func.local_names_mut() {
                if !name.is_empty() {
                    *name = get_next_name(&mut local_next_id);
                }
            }
        }
    }
}

impl MinifyNames {
    fn minify_local_names<'a>(
        &self,
        func: &mut crate::ir::module::Function<'a>,
        next_id: &mut usize,
        name_map: &mut HashMap<String, String>,
        _allocator: &'a bumpalo::Bump,
    ) {
        if let Some(mut body) = func.body() {
            let mut collector = BranchNameCollector { next_id, name_map };
            collector.visit(&mut body);
        }
    }
}

struct BranchNameCollector<'map> {
    next_id: &'map mut usize,
    name_map: &'map mut HashMap<String, String>,
}

impl<'a, 'map> Visitor<'a> for BranchNameCollector<'map> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        match &mut expr.kind {
            ExpressionKind::Block { name, .. } | ExpressionKind::Loop { name, .. } => {
                if let Some(old_name) = name {
                    if !self.name_map.contains_key(*old_name) {
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
                        self.name_map.insert((*old_name).to_string(), gen_name);
                    }
                }
            }
            _ => {}
        }
        self.visit_children(expr);
    }
}

struct BranchNameUpdater<'map, 'a> {
    name_map: &'map HashMap<String, String>,
    allocator: &'a bumpalo::Bump,
}

impl<'map, 'a> Visitor<'a> for BranchNameUpdater<'map, 'a> {
    fn visit_expression(&mut self, expr: &mut ExprRef<'a>) {
        match &mut expr.kind {
            ExpressionKind::Block { name, .. } | ExpressionKind::Loop { name, .. } => {
                if let Some(old_name) = name {
                    if let Some(new_name) = self.name_map.get(*old_name) {
                        *name = Some(self.allocator.alloc_str(new_name));
                    }
                }
            }
            ExpressionKind::Break { name, .. } => {
                if let Some(new_name) = self.name_map.get(*name) {
                    *name = self.allocator.alloc_str(new_name);
                }
            }
            ExpressionKind::Switch { names, default, .. } => {
                for target in names.iter_mut() {
                    if let Some(new_name) = self.name_map.get(*target) {
                        *target = self.allocator.alloc_str(new_name);
                    }
                }
                if let Some(new_name) = self.name_map.get(*default) {
                    *default = self.allocator.alloc_str(new_name);
                }
            }
            _ => {}
        }
        self.visit_children(expr);
    }
}

/// Removes all names from the module where possible.
pub struct StripNames;

impl Pass for StripNames {
    fn name(&self) -> &str {
        "StripNames"
    }

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        // Clear all debug names
        for (_, func) in &mut module.functions {
            func.name = String::new();
            for name in func.local_names_mut() {
                name.clear();
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

    fn run<'a>(&mut self, module: &mut Module<'a>) {
        let mut next_id = 0;
        for export in &mut module.exports {
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
            next_id += 1;
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

    fn run<'a>(&mut self, _module: &mut Module<'a>) {
        // TODO: Re-implement using new import architecture
    }
}
