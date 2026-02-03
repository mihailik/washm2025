use crate::collections::IdArena;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, ExpressionMarker};
use crate::ir::module::{ExportKind, Function, Module};
use crate::ir::visitor::ReadOnlyVisitor;
use crate::ir::{LocalId, Type};
use std::collections::HashSet;

pub struct Validator<'m> {
    module: &'m Module,
    current_function: Option<&'m Function>,
    valid: bool,
    errors: Vec<String>,
}

impl<'m> Validator<'m> {
    pub fn new(module: &'m Module) -> Self {
        Self {
            module,
            current_function: None,
            valid: true,
            errors: Vec::new(),
        }
    }

    pub fn validate(mut self) -> (bool, Vec<String>) {
        // Validate each function
        for (func_id, func) in self.module.functions.iter() {
            self.current_function = Some(func);
            let context = format!("Function '{}' (ID {:?}): ", func.name, func_id);

            // Validate type_idx if present
            if let Some(type_id) = func.type_idx {
                if let Some(func_type) = self.module.types.get(type_id) {
                    // Verify that the function signature matches the type
                    if func_type.params != func.params || func_type.results != func.results {
                        self.fail(&format!(
                            "{}Signature mismatch with type {:?}. Expected ({:?} -> {:?}), got ({:?} -> {:?})",
                            context, type_id, func_type.params, func_type.results, func.params, func.results
                        ));
                    }
                } else {
                    self.fail(&format!(
                        "{}Type index {:?} does not exist",
                        context, type_id,
                    ));
                }
            }

            // Check body if present
            if let Some(body_id) = func.body() {
                self.visit(body_id, &self.module.expressions);

                // Check return type
                let body = &self.module.expressions[body_id];
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
                ExportKind::Function(id) => {
                    if self.module.functions.get(id).is_none() {
                        self.fail(&format!("Exported function {:?} does not exist", id));
                    }
                }
                ExportKind::Global(id) => {
                    if self.module.globals.get(id).is_none() {
                        self.fail(&format!("Exported global {:?} does not exist", id));
                    }
                }
                ExportKind::Memory(id) => {
                    if self.module.memories.get(id).is_none() {
                        self.fail(&format!("Exported memory {:?} does not exist", id));
                    }
                }
                ExportKind::Table(id) => {
                    if self.module.tables.get(id).is_none() {
                        self.fail(&format!("Exported table {:?} does not exist", id));
                    }
                }
            }
        }

        // Validate data segments
        for (_, segment) in &self.module.data {
            // Check memory exists
            if self.module.memories.get(segment.memory_index).is_none() {
                self.fail(&format!(
                    "Data segment references memory {:?}, but it does not exist",
                    segment.memory_index
                ));
            }
            // Validate offset expression (must be constant)
            // For now, just walk the expression to trigger validation
            self.visit(segment.offset, &self.module.expressions);
        }

        // Validate start function
        if let Some(start_id) = self.module.start {
            if let Some(func) = self.module.functions.get(start_id) {
                // Check that start function has no parameters and no results
                if func.params != Type::NONE || func.results != Type::NONE {
                    self.fail(&format!(
                        "Start function {:?} must have no parameters and no results, but has ({:?} -> {:?})",
                        start_id, func.params, func.results
                    ));
                }
            } else {
                self.fail(&format!("Start function {:?} does not exist", start_id));
            }
        }

        // Validate element segments
        for (_, segment) in &self.module.elements {
            // Check table exists
            if self.module.tables.get(segment.table_index).is_none() {
                self.fail(&format!(
                    "Element segment references table {:?}, but it does not exist",
                    segment.table_index
                ));
            }

            // Validate function IDs
            for &func_id in &segment.func_indices {
                if self.module.functions.get(func_id).is_none() {
                    self.fail(&format!(
                        "Element segment has function ID {:?} which does not exist",
                        func_id
                    ));
                }
            }

            // Validate offset expression (must be constant)
            self.visit(segment.offset, &self.module.expressions);
        }

        (self.valid, self.errors)
    }

    fn fail(&mut self, msg: &str) {
        self.valid = false;
        self.errors.push(msg.to_string());
    }

    fn get_local_type(&self, func: &Function, id: LocalId) -> Option<Type> {
        func.locals().get(id.cast()).map(|l| l.type_)
    }
}

impl<'m> ReadOnlyVisitor for Validator<'m> {
    fn visit_expression(
        &mut self,
        expr_id: ExpressionId,
        arena: &IdArena<ExpressionMarker, Expression>,
    ) {
        let expr = &arena[expr_id];
        match &expr.kind {
            ExpressionKind::Binary { op, left, right } => {
                let left_expr = &arena[*left];
                let right_expr = &arena[*right];
                if left_expr.type_ != right_expr.type_
                    && left_expr.type_ != Type::UNREACHABLE
                    && right_expr.type_ != Type::UNREACHABLE
                {
                    self.fail(&format!(
                        "Binary op {:?} operands type mismatch: {:?} vs {:?}",
                        op, left_expr.type_, right_expr.type_
                    ));
                }
            }
            ExpressionKind::LocalGet { id } => {
                if let Some(current) = self.current_function {
                    if let Some(expected_ty) = self.get_local_type(current, *id) {
                        if expr.type_ != expected_ty && expr.type_ != Type::UNREACHABLE {
                            self.fail(&format!(
                                "LocalGet: Expression type {:?} does not match local {:?} type {:?}",
                                expr.type_, id, expected_ty
                            ));
                        }
                    } else {
                        self.fail(&format!("LocalGet: ID {:?} out of bounds", id));
                    }
                }
            }
            ExpressionKind::LocalSet { id, value } | ExpressionKind::LocalTee { id, value } => {
                if let Some(current) = self.current_function {
                    if let Some(expected_ty) = self.get_local_type(current, *id) {
                        let val_expr = &arena[*value];
                        if val_expr.type_ != expected_ty && val_expr.type_ != Type::UNREACHABLE {
                            self.fail(&format!(
                                "LocalSet/Tee: Value type {:?} does not match local {:?} type {:?}",
                                val_expr.type_, id, expected_ty
                            ));
                        }
                    } else {
                        self.fail(&format!("LocalSet/Tee: ID {:?} out of bounds", id));
                    }
                }
            }
            ExpressionKind::GlobalGet { id } => {
                if let Some(g) = self.module.globals.get(*id) {
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
                if let Some(g) = self.module.globals.get(*id) {
                    if !g.mutable {
                        self.fail(&format!("GlobalSet: Global {:?} is immutable", id));
                    }
                    let val_expr = &arena[*value];
                    if val_expr.type_ != g.type_ && val_expr.type_ != Type::UNREACHABLE {
                        self.fail(&format!(
                            "GlobalSet: Value type {:?} does not match global type {:?}",
                            val_expr.type_, g.type_
                        ));
                    }
                } else {
                    self.fail(&format!("GlobalSet: Invalid ID {:?}", id));
                }
            }
            ExpressionKind::Call {
                target, operands, ..
            } => {
                if let Some(func) = self.module.functions.get(*target) {
                    if !operands.is_empty() {
                        let first_op_expr = &arena[operands[0]];
                        if first_op_expr.type_ != func.params
                            && first_op_expr.type_ != Type::UNREACHABLE
                        {
                            self.fail(&format!("Call to {:?} param mismatch", target));
                        }
                    }
                } else {
                    self.fail(&format!("Call target not found: {:?}", target));
                }
            }
            ExpressionKind::CallIndirect { table, .. } => {
                if self.module.tables.get(*table).is_none() {
                    self.fail(&format!("CallIndirect: Table {:?} does not exist", table));
                }
            }
            ExpressionKind::MemorySize { memory } => {
                if self.module.memories.get(*memory).is_none() {
                    self.fail(&format!("MemorySize: Memory {:?} does not exist", memory));
                }
            }
            ExpressionKind::MemoryGrow { delta, memory } => {
                if self.module.memories.get(*memory).is_none() {
                    self.fail(&format!("MemoryGrow: Memory {:?} does not exist", memory));
                }
                let delta_expr = &arena[*delta];
                if delta_expr.type_ != Type::I32 && delta_expr.type_ != Type::UNREACHABLE {
                    self.fail(&format!(
                        "MemoryGrow: Delta type {:?} must be I32",
                        delta_expr.type_
                    ));
                }
            }
            ExpressionKind::MemoryCopy {
                dest_memory,
                src_memory,
                ..
            } => {
                if self.module.memories.get(*dest_memory).is_none() {
                    self.fail(&format!(
                        "MemoryCopy: Dest memory {:?} does not exist",
                        dest_memory
                    ));
                }
                if self.module.memories.get(*src_memory).is_none() {
                    self.fail(&format!(
                        "MemoryCopy: Src memory {:?} does not exist",
                        src_memory
                    ));
                }
            }
            ExpressionKind::MemoryFill { memory, .. } => {
                if self.module.memories.get(*memory).is_none() {
                    self.fail(&format!("MemoryFill: Memory {:?} does not exist", memory));
                }
            }
            ExpressionKind::AtomicNotify { memory, .. } => {
                if self.module.memories.get(*memory).is_none() {
                    self.fail(&format!("AtomicNotify: Memory {:?} does not exist", memory));
                }
            }
            _ => {}
        }
        self.visit_children(expr_id, arena);
    }
}
