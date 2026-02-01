use crate::ir::expression::{ExprRef, ExpressionKind};
use crate::ir::module::{ExportKind, Function, Module};
use crate::ir::visitor::ReadOnlyVisitor;
use crate::ir::Type;
use std::collections::HashSet;

pub struct Validator<'a, 'm> {
    module: &'m Module<'a>,
    current_function: Option<&'m Function<'a>>,
    valid: bool,
    errors: Vec<String>,
    // Caches for index spaces
    func_imports: Vec<&'m crate::ir::module::Import>,
    memory_import: Option<&'m crate::ir::module::Import>,
}

impl<'a, 'm> Validator<'a, 'm> {
    pub fn new(module: &'m Module<'a>) -> Self {
        let mut func_imports = Vec::new();
        let mut memory_import = None;

        for import in &module.imports {
            match import.kind {
                crate::ir::module::ImportKind::Function(..) => func_imports.push(import),
                crate::ir::module::ImportKind::Memory(..) => memory_import = Some(import),
                _ => {}
            }
        }

        Self {
            module,
            current_function: None,
            valid: true,
            errors: Vec::new(),
            func_imports,
            memory_import,
        }
    }

    pub fn validate(mut self) -> (bool, Vec<String>) {
        // Validate each function
        for (func_idx, func) in self.module.functions.iter().enumerate() {
            self.current_function = Some(func);
            let context = format!("Function '{}' (index {}): ", func.name, func_idx);

            // Validate type_idx if present
            if let Some(type_idx) = func.type_idx {
                if (type_idx as usize) >= self.module.types.len() {
                    self.fail(&format!(
                        "{}Type index {} out of bounds (module has {} types)",
                        context,
                        type_idx,
                        self.module.types.len()
                    ));
                } else {
                    // Verify that the function signature matches the type
                    let func_type = &self.module.types[type_idx as usize];
                    if func_type.params != func.params || func_type.results != func.results {
                        self.fail(&format!(
                            "{}Signature mismatch with type {}. Expected ({:?} -> {:?}), got ({:?} -> {:?})",
                            context, type_idx, func_type.params, func_type.results, func.params, func.results
                        ));
                    }
                }
            }

            // Check body if present
            if let Some(body) = &func.body {
                self.visit(*body);

                // Check return type
                if body.type_ != func.results {
                    // Simple check: Allow Unreachable
                    if body.type_ != Type::UNREACHABLE {
                        self.fail(&format!(
                            "{}Result mismatch. Expected {:?}, got {:?}",
                            context, func.results, body.type_
                        ));
                    }
                }
            }
        }

        // Validate exports
        let mut export_names = HashSet::new();
        for export in &self.module.exports {
            if !export_names.insert(&export.name) {
                self.fail(&format!("Duplicate export name: {}", export.name));
            }

            match export.kind {
                ExportKind::Function(idx) => {
                    let total_funcs = self.func_imports.len() + self.module.functions.len();
                    if (idx as usize) >= total_funcs {
                        self.fail(&format!("Exported function index {} out of bounds", idx));
                    }
                }
                ExportKind::Global(id) => {
                    if self.module.globals.get(id.cast()).is_none() {
                        self.fail(&format!("Exported global {:?} does not exist", id));
                    }
                }
                ExportKind::Memory(idx) => {
                    let has_memory = self.memory_import.is_some() || self.module.memory.is_some();
                    if !has_memory {
                        self.fail("Exported memory but no memory exists");
                    } else if idx != 0 {
                        self.fail(&format!(
                            "Exported memory index {} out of bounds (only 0 allowed)",
                            idx
                        ));
                    }
                }
                ExportKind::Table(idx) => {
                    let has_table = self.module.table.is_some()
                        || self.module.imports.iter().any(|imp| {
                            matches!(imp.kind, crate::ir::module::ImportKind::Table(..))
                        });

                    if !has_table {
                        self.fail("Exported table but no table exists");
                    } else if idx != 0 {
                        self.fail(&format!(
                            "Exported table index {} out of bounds (only 0 allowed)",
                            idx
                        ));
                    }
                }
            }
        }

        // Validate data segments
        for (i, segment) in self.module.data.iter().enumerate() {
            // Check memory exists
            let has_memory = self.memory_import.is_some() || self.module.memory.is_some();
            if !has_memory {
                self.fail(&format!(
                    "Data segment {} references memory, but no memory exists",
                    i
                ));
            }

            // Memory index must be 0 in MVP
            if segment.memory_index != 0 {
                self.fail(&format!(
                    "Data segment {} has invalid memory index {} (only 0 allowed)",
                    i, segment.memory_index
                ));
            }

            // Validate offset expression (must be constant)
            // For now, just walk the expression to trigger validation
            self.visit(segment.offset);
        }

        // Validate start function
        if let Some(start_idx) = self.module.start {
            let total_funcs = self.func_imports.len() + self.module.functions.len();
            if (start_idx as usize) >= total_funcs {
                self.fail(&format!(
                    "Start function index {} out of bounds (total functions: {})",
                    start_idx, total_funcs
                ));
            }

            // Check that start function has no parameters and no results
            let func_sig = if (start_idx as usize) < self.func_imports.len() {
                // It's an imported function
                let import = self.func_imports[start_idx as usize];
                if let crate::ir::module::ImportKind::Function(params, results) = import.kind {
                    Some((params, results))
                } else {
                    None
                }
            } else {
                // It's a defined function
                let local_idx = (start_idx as usize) - self.func_imports.len();
                self.module
                    .functions
                    .get(local_idx)
                    .map(|f| (f.params, f.results))
            };

            if let Some((params, results)) = func_sig {
                if params != Type::NONE {
                    self.fail("Start function must have no parameters");
                }
                if results != Type::NONE {
                    self.fail("Start function must have no results");
                }
            }
        }

        // Validate element segments
        for (i, segment) in self.module.elements.iter().enumerate() {
            // Check table exists
            let has_table = self.module.table.is_some()
                || self
                    .module
                    .imports
                    .iter()
                    .any(|imp| matches!(imp.kind, crate::ir::module::ImportKind::Table(..)));

            if !has_table {
                self.fail(&format!(
                    "Element segment {} references table, but no table exists",
                    i
                ));
            }

            // Table index must be 0 in MVP
            if segment.table_index != 0 {
                self.fail(&format!(
                    "Element segment {} has invalid table index {} (only 0 allowed)",
                    i, segment.table_index
                ));
            }

            // Validate function indices
            let total_funcs = self.func_imports.len() + self.module.functions.len();
            for &func_idx in &segment.func_indices {
                if (func_idx as usize) >= total_funcs {
                    self.fail(&format!(
                        "Element segment {} has function index {} out of bounds (total: {})",
                        i, func_idx, total_funcs
                    ));
                }
            }

            // Validate offset expression (must be constant)
            self.visit(segment.offset);
        }

        (self.valid, self.errors)
    }

    fn fail(&mut self, msg: &str) {
        self.valid = false;
        self.errors.push(msg.to_string());
    }

    fn get_local_type(&self, func: &Function, index: u32) -> Option<Type> {
        let idx = index as usize;
        let param_len = func.params.tuple_len();
        if idx < param_len {
            if param_len == 1 {
                return Some(func.params);
            }
            // Multi-value params
            if let Some(types) = crate::ir::type_store::lookup_tuple(func.params) {
                return types.get(idx).cloned();
            }
            return None;
        }

        let var_idx = idx - param_len;
        func.vars.get(var_idx).cloned()
    }
}

impl<'a, 'm> ReadOnlyVisitor<'a> for Validator<'a, 'm> {
    fn visit_expression(&mut self, expr: ExprRef<'a>) {
        match &expr.kind {
            ExpressionKind::Binary { op, left, right } => {
                if left.type_ != right.type_
                    && left.type_ != Type::UNREACHABLE
                    && right.type_ != Type::UNREACHABLE
                {
                    self.fail(&format!(
                        "Binary op {:?} operands type mismatch: {:?} vs {:?}",
                        op, left.type_, right.type_
                    ));
                }
            }
            ExpressionKind::LocalGet { index } => {
                if let Some(current) = self.current_function {
                    if let Some(expected_ty) = self.get_local_type(current, *index) {
                        if expr.type_ != expected_ty && expr.type_ != Type::UNREACHABLE {
                            self.fail(&format!(
                                "LocalGet: Expression type {:?} does not match local {} type {:?}",
                                expr.type_, index, expected_ty
                            ));
                        }
                    } else {
                        self.fail(&format!("LocalGet: Index {} out of bounds", index));
                    }
                }
            }
            ExpressionKind::LocalSet { index, value }
            | ExpressionKind::LocalTee { index, value } => {
                if let Some(current) = self.current_function {
                    if let Some(expected_ty) = self.get_local_type(current, *index) {
                        if value.type_ != expected_ty && value.type_ != Type::UNREACHABLE {
                            self.fail(&format!(
                                "LocalSet/Tee: Value type {:?} does not match local {} type {:?}",
                                value.type_, index, expected_ty
                            ));
                        }
                    } else {
                        self.fail(&format!("LocalSet/Tee: Index {} out of bounds", index));
                    }
                }
            }
            ExpressionKind::GlobalGet { id } => {
                if let Some(g) = self.module.globals.get(id.cast()) {
                    if expr.type_ != g.type_ && expr.type_ != Type::UNREACHABLE {
                        self.fail(&format!(
                            "GlobalGet: Expression type {:?} does not match global type {:?}",
                            expr.type_, g.type_
                        ));
                    }
                } else {
                    self.fail(&format!("GlobalGet: Invalid ID {:?}", id));
                }
            }
            ExpressionKind::GlobalSet { id, value } => {
                if let Some(g) = self.module.globals.get(id.cast()) {
                    if !g.mutable {
                        self.fail(&format!("GlobalSet: Global {:?} is immutable", id));
                    }
                    if value.type_ != g.type_ && value.type_ != Type::UNREACHABLE {
                        self.fail(&format!(
                            "GlobalSet: Value type {:?} does not match global type {:?}",
                            value.type_, g.type_
                        ));
                    }
                } else {
                    self.fail(&format!("GlobalSet: Invalid ID {:?}", id));
                }
            }
            ExpressionKind::Call {
                target, operands, ..
            } => {
                let sig = if let Some(func) = self.module.get_function(target) {
                    Some((func.params, func.results))
                } else {
                    self.func_imports
                        .iter()
                        .find(|import| import.name == *target) // Assuming field name == internal name
                        .and_then(|import| {
                            if let crate::ir::module::ImportKind::Function(params, results) =
                                import.kind
                            {
                                Some((params, results))
                            } else {
                                None
                            }
                        })
                };

                if let Some((params, _results)) = sig {
                    // Check params
                    // Limitation: Type is single value. If mismatch, fail.
                    // If multiple operands, we need tuple support in Type.
                    // For now, simple check.
                    if !operands.is_empty() {
                        // If we have operands but params is valid (not NONE), check type.
                        // Assuming 1 param only for now as per minimal parser type support.
                        let op_type = operands[0].type_;
                        if op_type != params && op_type != Type::UNREACHABLE {
                            self.fail(&format!("Call to {} param mismatch", target));
                        }
                    }
                } else {
                    self.fail(&format!("Call target not found: {}", target));
                }
            }
            ExpressionKind::Return { .. }
            | ExpressionKind::Unreachable
            | ExpressionKind::Drop { .. }
            | ExpressionKind::Select { .. }
            | ExpressionKind::Load { .. }
            | ExpressionKind::Store { .. }
            | ExpressionKind::Const(_)
            | ExpressionKind::Unary { .. }
            | ExpressionKind::Block { .. }
            | ExpressionKind::If { .. }
            | ExpressionKind::Loop { .. }
            | ExpressionKind::Break { .. }
            | ExpressionKind::Nop
            | ExpressionKind::Switch { .. }
            | ExpressionKind::CallIndirect { .. }
            | ExpressionKind::MemoryGrow { .. }
            | ExpressionKind::MemorySize
            | ExpressionKind::AtomicRMW { .. }
            | ExpressionKind::AtomicCmpxchg { .. }
            | ExpressionKind::AtomicWait { .. }
            | ExpressionKind::AtomicNotify { .. }
            | ExpressionKind::AtomicFence
            | ExpressionKind::SIMDExtract { .. }
            | ExpressionKind::SIMDReplace { .. }
            | ExpressionKind::SIMDShuffle { .. }
            | ExpressionKind::SIMDTernary { .. }
            | ExpressionKind::SIMDShift { .. }
            | ExpressionKind::SIMDLoad { .. }
            | ExpressionKind::SIMDLoadStoreLane { .. }
            | ExpressionKind::MemoryInit { .. }
            | ExpressionKind::DataDrop { .. }
            | ExpressionKind::MemoryCopy { .. }
            | ExpressionKind::MemoryFill { .. }
            | ExpressionKind::TableGet { .. }
            | ExpressionKind::TableSet { .. }
            | ExpressionKind::TableSize { .. }
            | ExpressionKind::TableGrow { .. }
            | ExpressionKind::TableFill { .. }
            | ExpressionKind::TableCopy { .. }
            | ExpressionKind::TableInit { .. }
            | ExpressionKind::RefNull { .. }
            | ExpressionKind::RefIsNull { .. }
            | ExpressionKind::RefAs { .. }
            | ExpressionKind::RefEq { .. }
            | ExpressionKind::RefFunc { .. }
            | ExpressionKind::StructNew { .. }
            | ExpressionKind::StructGet { .. }
            | ExpressionKind::StructSet { .. }
            | ExpressionKind::ArrayNew { .. }
            | ExpressionKind::ArrayGet { .. }
            | ExpressionKind::ArraySet { .. }
            | ExpressionKind::ArrayLen { .. }
            | ExpressionKind::ElemDrop { .. }
            | ExpressionKind::Try { .. }
            | ExpressionKind::Throw { .. }
            | ExpressionKind::Rethrow { .. }
            | ExpressionKind::TupleMake { .. }
            | ExpressionKind::TupleExtract { .. }
            | ExpressionKind::Pop { .. }
            | ExpressionKind::I31New { .. }
            | ExpressionKind::I31Get { .. }
            | ExpressionKind::RefTest { .. }
            | ExpressionKind::RefCast { .. }
            | ExpressionKind::BrOn { .. } => {
                // These expression kinds don't require special validation yet
            }
        }
    }
}
