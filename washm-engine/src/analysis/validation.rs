use crate::ir::expression::{ExprRef, ExpressionKind};
use crate::ir::module::{ExportKind, Function, Module};
use crate::ir::visitor::ReadOnlyVisitor;
use crate::ir::{LocalId, Type};
use std::collections::HashSet;

pub struct Validator<'a, 'm> {
    module: &'m Module<'a>,
    current_function: Option<&'m Function<'a>>,
    valid: bool,
    errors: Vec<String>,
}

impl<'a, 'm> Validator<'a, 'm> {
    pub fn new(module: &'m Module<'a>) -> Self {
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
            if let Some(body) = func.body() {
                if !body.is_null() {
                    self.visit(body);

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
            self.visit(segment.offset);
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
            self.visit(segment.offset);
        }

        (self.valid, self.errors)
    }

    fn fail(&mut self, msg: &str) {
        self.valid = false;
        self.errors.push(msg.to_string());
    }

    fn get_local_type(&self, func: &Function, id: LocalId) -> Option<Type> {
        if func.is_import() {
            return None;
        }
        func.locals().get(id.cast()).map(|l| l.type_)
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
                        if value.type_ != expected_ty && value.type_ != Type::UNREACHABLE {
                            self.fail(&format!(
                                "LocalSet/Tee: Value type {:?} does not match local {:?} type {:?}",
                                value.type_, id, expected_ty
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
                if let Some(func) = self.module.functions.get(*target) {
                    if !operands.is_empty() {
                        let op_type = operands[0].type_;
                        if op_type != func.params && op_type != Type::UNREACHABLE {
                            self.fail(&format!("Call to {:?} param mismatch", target));
                        }
                    }
                } else {
                    self.fail(&format!("Call target not found: {:?}", target));
                }
            }
            ExpressionKind::CallIndirect { table, .. } => {
                // Here 'type_' in CallIndirect is the Type (signature) itself,
                // but we should check if it's a valid signature in the module if possible.
                // However, the IR 'type_' is the actual Type object.
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
                if delta.type_ != Type::I32 && delta.type_ != Type::UNREACHABLE {
                    self.fail(&format!(
                        "MemoryGrow: Delta type {:?} must be I32",
                        delta.type_
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
            | ExpressionKind::AtomicRMW { .. }
            | ExpressionKind::AtomicCmpxchg { .. }
            | ExpressionKind::AtomicWait { .. }
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
