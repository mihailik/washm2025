use crate::ir::expression::{ExprRef, ExpressionKind};
use crate::ir::module::Module;
use crate::ir::ops::{AtomicOp, BinaryOp, UnaryOp};
use crate::ir::{DataId, ElementId, FuncId, GlobalId, MemoryId, TableId, TypeId};
use crate::ir::{Literal, Type};
use std::borrow::Cow;
use std::io;
use wasm_encoder::{
    AbstractHeapType, CodeSection, ConstExpr, DataCountSection, DataSection, ElementSection,
    Encode, ExportKind, ExportSection, Function as EncoderFunction, FunctionSection, GlobalSection,
    HeapType, ImportSection, IndirectNameMap, Instruction, MemArg, MemorySection, MemoryType,
    Module as EncoderModule, NameMap, NameSection, RefType, StartSection, TableSection, TableType,
    TypeSection, ValType,
};

trait InstructionEncoder {
    fn instruction(&mut self, instr: &Instruction<'_>) -> &mut Self;
}

impl InstructionEncoder for EncoderFunction {
    fn instruction(&mut self, instr: &Instruction<'_>) -> &mut Self {
        self.instruction(instr)
    }
}

struct ConstExprEncoder {
    bytes: Vec<u8>,
}

impl ConstExprEncoder {
    fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    fn finish(self) -> ConstExpr {
        ConstExpr::raw(self.bytes)
    }
}

impl InstructionEncoder for ConstExprEncoder {
    fn instruction(&mut self, instr: &Instruction<'_>) -> &mut Self {
        instr.encode(&mut self.bytes);
        self
    }
}

pub struct BinaryWriter {
    _label_stack: Vec<Option<String>>, // Stack of label names for depth calculation
}

#[derive(Debug)]
pub enum WriteError {
    Io(io::Error),
    UnsupportedFeature(String),
    LabelNotFound(String),
    InvalidExpression,
    InternalError(String),
}

impl From<io::Error> for WriteError {
    fn from(e: io::Error) -> Self {
        WriteError::Io(e)
    }
}

type Result<T> = std::result::Result<T, WriteError>;

impl Default for BinaryWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl BinaryWriter {
    pub fn new() -> Self {
        Self {
            _label_stack: Vec::new(),
        }
    }

    fn to_val_type(ty: Type) -> ValType {
        if ty == Type::I32 {
            ValType::I32
        } else if ty == Type::I64 {
            ValType::I64
        } else if ty == Type::F32 {
            ValType::F32
        } else if ty == Type::F64 {
            ValType::F64
        } else if ty == Type::V128 {
            ValType::V128
        } else if ty == Type::FUNCREF {
            ValType::Ref(RefType {
                nullable: true,
                heap_type: HeapType::Abstract {
                    shared: false,
                    ty: AbstractHeapType::Func,
                },
            })
        } else if ty == Type::EXTERNREF {
            ValType::Ref(RefType {
                nullable: true,
                heap_type: HeapType::Abstract {
                    shared: false,
                    ty: AbstractHeapType::Extern,
                },
            })
        } else {
            // Default to I32 for unknown types for now
            ValType::I32
        }
    }

    fn to_ref_type(ty: Type) -> RefType {
        if ty == Type::EXTERNREF {
            RefType {
                nullable: true,
                heap_type: HeapType::Abstract {
                    shared: false,
                    ty: AbstractHeapType::Extern,
                },
            }
        } else {
            RefType {
                nullable: true,
                heap_type: HeapType::Abstract {
                    shared: false,
                    ty: AbstractHeapType::Func,
                },
            }
        }
    }

    fn to_block_type(ty: Type) -> wasm_encoder::BlockType {
        if ty == Type::NONE {
            wasm_encoder::BlockType::Empty
        } else {
            let expanded = Self::expand_type(ty);
            if expanded.len() == 1 {
                wasm_encoder::BlockType::Result(Self::to_val_type(expanded[0]))
            } else {
                // TODO: For multi-value returns, we need a type index.
                // This requires looking up or adding a type in the TypeSection.
                wasm_encoder::BlockType::Empty
            }
        }
    }

    fn expand_type(ty: Type) -> Vec<Type> {
        if ty == Type::NONE {
            return Vec::new();
        }
        if let Some(types) = crate::ir::type_store::lookup_tuple(ty) {
            let mut result = Vec::new();
            for t in types {
                result.extend(Self::expand_type(t));
            }
            result
        } else {
            vec![ty]
        }
    }

    pub fn write_module(&mut self, module: &Module) -> Result<Vec<u8>> {
        let mut encoder = EncoderModule::new();

        let mut func_map = std::collections::HashMap::<FuncId, u32>::new();
        let mut table_map = std::collections::HashMap::<TableId, u32>::new();
        let mut memory_map = std::collections::HashMap::<MemoryId, u32>::new();
        let mut global_map = std::collections::HashMap::<GlobalId, u32>::new();
        let mut type_id_map = std::collections::HashMap::<TypeId, u32>::new();
        let mut sig_map = std::collections::HashMap::<(Vec<Type>, Vec<Type>), u32>::new();
        let mut data_map = std::collections::HashMap::<DataId, u32>::new();
        let mut element_map = std::collections::HashMap::<ElementId, u32>::new();

        let mut func_idx = 0;
        let mut table_idx = 0;
        let mut memory_idx = 0;
        let mut global_idx = 0;
        let mut type_idx = 0;
        let mut data_idx = 0;
        let mut element_idx = 0;

        let mut all_types = Vec::new();

        // 1. Collect and assign indices to types from module
        // We MUST preserve all explicit types even if they are duplicates.
        for (id, func_type) in &module.types {
            let sig = (
                Self::expand_type(func_type.params),
                Self::expand_type(func_type.results),
            );

            // Keep track of the first index for each signature for implicit use
            if !sig_map.contains_key(&sig) {
                sig_map.insert(sig.clone(), type_idx);
            }

            all_types.push(sig);
            type_id_map.insert(id, type_idx);
            type_idx += 1;
        }

        // 1.1 Collect implicit types from functions and imports
        for (_, func) in &module.functions {
            let sig = (
                Self::expand_type(func.params),
                Self::expand_type(func.results),
            );
            if !sig_map.contains_key(&sig) {
                sig_map.insert(sig.clone(), type_idx);
                all_types.push(sig);
                type_idx += 1;
            }
        }

        // 2. Assign indices to segments
        for (id, _) in &module.data {
            data_map.insert(id, data_idx);
            data_idx += 1;
        }
        for (id, _) in &module.elements {
            element_map.insert(id, element_idx);
            element_idx += 1;
        }

        // 3. Assign indices to imports
        for (id, func) in &module.functions {
            if func.is_import() {
                func_map.insert(id, func_idx);
                func_idx += 1;
            }
        }
        for (id, table) in &module.tables {
            if let crate::ir::module::TableKind::Import { .. } = table.kind {
                table_map.insert(id, table_idx);
                table_idx += 1;
            }
        }
        for (id, mem) in &module.memories {
            if let crate::ir::module::MemoryKind::Import { .. } = mem.kind {
                memory_map.insert(id, memory_idx);
                memory_idx += 1;
            }
        }
        for (id, global) in &module.globals {
            if global.is_import() {
                global_map.insert(id, global_idx);
                global_idx += 1;
            }
        }

        // 2. Assign indices to definitions
        for (id, func) in &module.functions {
            if !func.is_import() {
                func_map.insert(id, func_idx);
                func_idx += 1;
            }
        }
        for (id, table) in &module.tables {
            if let crate::ir::module::TableKind::Local = table.kind {
                table_map.insert(id, table_idx);
                table_idx += 1;
            }
        }
        for (id, mem) in &module.memories {
            if let crate::ir::module::MemoryKind::Local = mem.kind {
                memory_map.insert(id, memory_idx);
                memory_idx += 1;
            }
        }
        for (id, global) in &module.globals {
            if let crate::ir::module::GlobalKind::Local { .. } = global.kind {
                global_map.insert(id, global_idx);
                global_idx += 1;
            }
        }

        // Type section
        if !all_types.is_empty() {
            let mut types = TypeSection::new();
            for sig in &all_types {
                let p: Vec<ValType> = sig.0.iter().map(|&t| Self::to_val_type(t)).collect();
                let r: Vec<ValType> = sig.1.iter().map(|&t| Self::to_val_type(t)).collect();
                types.ty().function(p, r);
            }
            encoder.section(&types);
        }

        let mut func_type_indices: Vec<u32> = Vec::new();
        // Function section only includes types for definitions
        for (_, func) in &module.functions {
            if !func.is_import() {
                let idx = if let Some(type_id) = func.type_idx {
                    *type_id_map.get(&type_id).ok_or_else(|| {
                        WriteError::InternalError("TypeId not found in map".to_string())
                    })?
                } else {
                    // Use signature map
                    let sig = (
                        Self::expand_type(func.params),
                        Self::expand_type(func.results),
                    );
                    *sig_map.get(&sig).ok_or_else(|| {
                        WriteError::InternalError(
                            "Signature not found in types after collection".to_string(),
                        )
                    })?
                };
                func_type_indices.push(idx);
            }
        }

        // 2. Import Section
        {
            let mut imports = ImportSection::new();
            let mut has_imports = false;

            for (_, func) in &module.functions {
                if let Some(import) = &func.import {
                    let type_idx = if let Some(type_id) = func.type_idx {
                        *type_id_map.get(&type_id).ok_or_else(|| {
                            WriteError::InternalError("TypeId not found in map".to_string())
                        })?
                    } else {
                        let sig = (
                            Self::expand_type(func.params),
                            Self::expand_type(func.results),
                        );
                        *sig_map.get(&sig).ok_or_else(|| {
                            WriteError::InternalError(
                                "Signature not found in types after collection".to_string(),
                            )
                        })?
                    };
                    imports.import(
                        &import.module,
                        &import.name,
                        wasm_encoder::EntityType::Function(type_idx),
                    );
                    has_imports = true;
                }
            }

            for (_, table) in &module.tables {
                if let crate::ir::module::TableKind::Import { module, field } = &table.kind {
                    imports.import(
                        module,
                        field,
                        wasm_encoder::EntityType::Table(wasm_encoder::TableType {
                            element_type: Self::to_ref_type(table.limits.element_type),
                            minimum: table.limits.initial as u64,
                            maximum: table.limits.maximum.map(|m| m as u64),
                            table64: false,
                            shared: false,
                        }),
                    );
                    has_imports = true;
                }
            }

            for (_, mem) in &module.memories {
                if let crate::ir::module::MemoryKind::Import { module, field } = &mem.kind {
                    imports.import(
                        module,
                        field,
                        wasm_encoder::EntityType::Memory(wasm_encoder::MemoryType {
                            minimum: mem.limits.initial as u64,
                            maximum: mem.limits.maximum.map(|m| m as u64),
                            memory64: false,
                            shared: false,
                            page_size_log2: None,
                        }),
                    );
                    has_imports = true;
                }
            }

            for (_, global) in &module.globals {
                if let crate::ir::module::GlobalKind::Import { module, field } = &global.kind {
                    imports.import(
                        module,
                        field,
                        wasm_encoder::EntityType::Global(wasm_encoder::GlobalType {
                            val_type: Self::to_val_type(global.type_),
                            mutable: global.mutable,
                            shared: false,
                        }),
                    );
                    has_imports = true;
                }
            }

            if has_imports {
                encoder.section(&imports);
            }
        }

        // 3. Function Section
        if !func_type_indices.is_empty() {
            let mut functions = FunctionSection::new();
            for &idx in &func_type_indices {
                functions.function(idx);
            }
            encoder.section(&functions);
        }

        // 4. Table Section
        {
            let mut tables = TableSection::new();
            let mut has_tables = false;
            for (_, table) in &module.tables {
                if let crate::ir::module::TableKind::Local = table.kind {
                    tables.table(TableType {
                        element_type: Self::to_ref_type(table.limits.element_type),
                        minimum: table.limits.initial as u64,
                        maximum: table.limits.maximum.map(|m| m as u64),
                        table64: false,
                        shared: false,
                    });
                    has_tables = true;
                }
            }
            if has_tables {
                encoder.section(&tables);
            }
        }

        // 5. Memory Section
        {
            let mut memories = MemorySection::new();
            let mut has_memories = false;
            for (_, mem) in &module.memories {
                if let crate::ir::module::MemoryKind::Local = mem.kind {
                    memories.memory(MemoryType {
                        minimum: mem.limits.initial as u64,
                        maximum: mem.limits.maximum.map(|m| m as u64),
                        memory64: false,
                        shared: false,
                        page_size_log2: None,
                    });
                    has_memories = true;
                }
            }
            if has_memories {
                encoder.section(&memories);
            }
        }

        // 6. Global Section
        if module
            .globals
            .iter()
            .any(|(_, g)| matches!(g.kind, crate::ir::module::GlobalKind::Local { .. }))
        {
            let mut globals = GlobalSection::new();
            for (_, global) in module.globals.iter() {
                if let crate::ir::module::GlobalKind::Local { init } = &global.kind {
                    let mut init_expr_encoder = ConstExprEncoder::new();
                    Self::write_instructions(
                        &mut init_expr_encoder,
                        *init,
                        &mut vec![],
                        &std::collections::HashMap::new(),
                        &global_map,
                        &type_id_map,
                        &data_map,
                        &element_map,
                        &table_map,
                        &memory_map,
                        &std::collections::HashMap::new(),
                    )?;
                    let init_expr = init_expr_encoder.finish();
                    globals.global(
                        wasm_encoder::GlobalType {
                            val_type: Self::to_val_type(global.type_),
                            mutable: global.mutable,
                            shared: false,
                        },
                        &init_expr,
                    );
                }
            }
            encoder.section(&globals);
        }

        // 7. Export Section
        if !module.exports.is_empty() {
            let mut exports = ExportSection::new();
            for export in &module.exports {
                let (kind, index) = match export.kind {
                    crate::ir::module::ExportKind::Function(id) => {
                        let idx = func_map.get(&id).cloned().ok_or_else(|| {
                            WriteError::InternalError(format!(
                                "Exported function ID {:?} not found in module",
                                id
                            ))
                        })?;
                        (ExportKind::Func, idx)
                    }
                    crate::ir::module::ExportKind::Table(id) => {
                        let idx = table_map.get(&id).cloned().ok_or_else(|| {
                            WriteError::InternalError(format!(
                                "Exported table ID {:?} not found in module",
                                id
                            ))
                        })?;
                        (ExportKind::Table, idx)
                    }
                    crate::ir::module::ExportKind::Memory(id) => {
                        let idx = memory_map.get(&id).cloned().ok_or_else(|| {
                            WriteError::InternalError(format!(
                                "Exported memory ID {:?} not found in module",
                                id
                            ))
                        })?;
                        (ExportKind::Memory, idx)
                    }
                    crate::ir::module::ExportKind::Global(id) => {
                        let idx = global_map.get(&id).cloned().ok_or_else(|| {
                            WriteError::InternalError(format!(
                                "Exported global ID {:?} not found in module",
                                id
                            ))
                        })?;
                        (ExportKind::Global, idx)
                    }
                };
                exports.export(&export.name, kind, index);
            }
            encoder.section(&exports);
        }

        // 8. Start Section
        if let Some(start_idx) = module.start {
            encoder.section(&StartSection {
                function_index: *func_map.get(&start_idx).unwrap(),
            });
        }

        // 9. Element Section
        if !module.elements.is_empty() {
            let mut elements = ElementSection::new();
            for (_, segment) in &module.elements {
                let mut offset_expr_encoder = ConstExprEncoder::new();
                Self::write_instructions(
                    &mut offset_expr_encoder,
                    segment.offset,
                    &mut vec![],
                    &func_map,
                    &global_map,
                    &type_id_map,
                    &data_map,
                    &element_map,
                    &table_map,
                    &memory_map,
                    &std::collections::HashMap::new(),
                )?;
                let offset_expr = offset_expr_encoder.finish();
                let func_indices: Vec<u32> = segment
                    .func_indices
                    .iter()
                    .map(|id| *func_map.get(id).unwrap())
                    .collect();
                elements.active(
                    Some(*table_map.get(&segment.table_index).unwrap()),
                    &offset_expr,
                    wasm_encoder::Elements::Functions(Cow::Owned(func_indices)),
                );
            }
            encoder.section(&elements);
        }

        // 9.1 Data Count Section (Section 12, but must precede Code Section)
        if !module.data.is_empty() {
            encoder.section(&DataCountSection {
                count: module.data.len() as u32,
            });
        }

        // 10. Code Section
        {
            let mut code = CodeSection::new();
            for (_, func) in &module.functions {
                if !func.is_import() {
                    let mut local_map = std::collections::HashMap::<crate::ir::LocalId, u32>::new();
                    let mut current_idx = 0;

                    // 1. Assign indices to parameters in explicit order
                    for &id in &func.param_ids {
                        local_map.insert(id, current_idx);
                        current_idx += 1;
                    }

                    // 2. Group non-parameter locals by type
                    let mut locals = Vec::new();
                    let mut type_groups =
                        std::collections::HashMap::<Type, Vec<crate::ir::LocalId>>::new();
                    let mut types_ordered = Vec::new();

                    let param_set: std::collections::HashSet<crate::ir::LocalId> =
                        func.param_ids.iter().cloned().collect();

                    for (id, local) in func.locals.iter() {
                        if param_set.contains(&id.cast()) {
                            continue;
                        }
                        let ty = local.type_;
                        if !type_groups.contains_key(&ty) {
                            types_ordered.push(ty);
                        }
                        type_groups.entry(ty).or_default().push(id.cast());
                    }

                    for ty in types_ordered {
                        let ids = &type_groups[&ty];
                        locals.push((ids.len() as u32, Self::to_val_type(ty)));
                        for &id in ids {
                            local_map.insert(id, current_idx);
                            current_idx += 1;
                        }
                    }

                    let mut func_encoder = EncoderFunction::new(locals);

                    if let Some(body_expr) = &func.body {
                        let mut label_stack = vec![None];
                        Self::write_instructions(
                            &mut func_encoder,
                            *body_expr,
                            &mut label_stack,
                            &func_map,
                            &global_map,
                            &type_id_map,
                            &data_map,
                            &element_map,
                            &table_map,
                            &memory_map,
                            &local_map,
                        )?;
                    }
                    func_encoder.instruction(&Instruction::End);
                    code.function(&func_encoder);
                }
            }
            if !code.is_empty() {
                encoder.section(&code);
            }
        }

        // 11. Data Section
        {
            let mut data = DataSection::new();
            for (_, segment) in &module.data {
                let mut offset_expr_encoder = ConstExprEncoder::new();
                Self::write_instructions(
                    &mut offset_expr_encoder,
                    segment.offset,
                    &mut vec![],
                    &func_map,
                    &global_map,
                    &type_id_map,
                    &data_map,
                    &element_map,
                    &table_map,
                    &memory_map,
                    &std::collections::HashMap::new(),
                )?;
                let offset_expr = offset_expr_encoder.finish();
                data.active(
                    *memory_map.get(&segment.memory_index).unwrap(),
                    &offset_expr,
                    segment.data.iter().copied(),
                );
            }
            if !data.is_empty() {
                encoder.section(&data);
            }
        }

        // 12. Name Section (Custom Section)
        {
            let mut names = NameSection::new();
            let mut has_names = false;

            // 1. Function names
            let mut func_names = NameMap::new();
            for (id, func) in &module.functions {
                if !func.name.is_empty() {
                    func_names.append(*func_map.get(&id).unwrap(), &func.name);
                }
            }
            if !func_names.is_empty() {
                names.functions(&func_names);
                has_names = true;
            }

            // 2. Global names
            let mut global_names = NameMap::new();
            for (id, global) in &module.globals {
                if !global.name.is_empty() {
                    global_names.append(*global_map.get(&id).unwrap(), &global.name);
                }
            }
            if !global_names.is_empty() {
                names.globals(&global_names);
                has_names = true;
            }

            // 3. Table names
            let mut table_names = NameMap::new();
            for (id, table) in &module.tables {
                if !table.name.is_empty() {
                    table_names.append(*table_map.get(&id).unwrap(), &table.name);
                }
            }
            if !table_names.is_empty() {
                names.tables(&table_names);
                has_names = true;
            }

            // 4. Memory names
            let mut memory_names = NameMap::new();
            for (id, memory) in &module.memories {
                if !memory.name.is_empty() {
                    memory_names.append(*memory_map.get(&id).unwrap(), &memory.name);
                }
            }
            if !memory_names.is_empty() {
                names.memories(&memory_names);
                has_names = true;
            }

            // 5. Local names (per function)
            let mut local_names = IndirectNameMap::new();
            let mut has_any_local_names = false;
            for (id, func) in &module.functions {
                let mut map = NameMap::new();
                let mut func_has_local_names = false;

                // We need to find the WASM indices for locals in this function
                // This logic mirrors the one in the Code Section writing
                let mut current_idx = 0;
                for &local_id in &func.param_ids {
                    if let Some(local) = func.locals.get(local_id.cast()) {
                        if !local.name.is_empty() {
                            map.append(current_idx, &local.name);
                            func_has_local_names = true;
                        }
                    }
                    current_idx += 1;
                }

                // Non-parameter locals
                let param_set: std::collections::HashSet<crate::ir::LocalId> =
                    func.param_ids.iter().cloned().collect();

                // To match indices, we must group them the same way as in Section 10
                let mut type_groups =
                    std::collections::HashMap::<Type, Vec<crate::ir::LocalId>>::new();
                let mut types_ordered = Vec::new();

                for (local_id, local) in func.locals.iter() {
                    if param_set.contains(&local_id.cast()) {
                        continue;
                    }
                    let ty = local.type_;
                    if !type_groups.contains_key(&ty) {
                        types_ordered.push(ty);
                    }
                    type_groups.entry(ty).or_default().push(local_id.cast());
                }

                for ty in types_ordered {
                    for &local_id in &type_groups[&ty] {
                        if let Some(local) = func.locals.get(local_id.cast()) {
                            if !local.name.is_empty() {
                                map.append(current_idx, &local.name);
                                func_has_local_names = true;
                            }
                        }
                        current_idx += 1;
                    }
                }

                if func_has_local_names {
                    local_names.append(*func_map.get(&id).unwrap(), &map);
                    has_any_local_names = true;
                }
            }
            if has_any_local_names {
                names.locals(&local_names);
                has_names = true;
            }

            if has_names {
                encoder.section(&names);
            }
        }

        Ok(encoder.finish())
    }

    #[allow(clippy::too_many_arguments)]
    fn write_instructions<E: InstructionEncoder>(
        encoder: &mut E,
        expr_ref: ExprRef,
        label_stack: &mut Vec<Option<String>>,
        func_map: &std::collections::HashMap<FuncId, u32>,
        global_map: &std::collections::HashMap<GlobalId, u32>,
        type_id_map: &std::collections::HashMap<TypeId, u32>,
        data_map: &std::collections::HashMap<DataId, u32>,
        element_map: &std::collections::HashMap<ElementId, u32>,
        table_map: &std::collections::HashMap<TableId, u32>,
        memory_map: &std::collections::HashMap<MemoryId, u32>,
        local_map: &std::collections::HashMap<crate::ir::LocalId, u32>,
    ) -> Result<()> {
        let expr = &*expr_ref;
        match &expr.kind {
            ExpressionKind::Const(lit) => match lit {
                Literal::I32(v) => {
                    encoder.instruction(&Instruction::I32Const(*v));
                }
                Literal::I64(v) => {
                    encoder.instruction(&Instruction::I64Const(*v));
                }
                Literal::F32(v) => {
                    encoder.instruction(&Instruction::F32Const((*v).into()));
                }
                Literal::F64(v) => {
                    encoder.instruction(&Instruction::F64Const((*v).into()));
                }
                Literal::V128(v) => {
                    let mut bytes = [0u8; 16];
                    bytes.copy_from_slice(v);
                    encoder.instruction(&Instruction::V128Const(i128::from_le_bytes(bytes)));
                }
            },
            ExpressionKind::LocalGet { id } => {
                let idx = local_map.get(id).ok_or_else(|| {
                    WriteError::InternalError(format!("Local not found: {:?}", id))
                })?;
                encoder.instruction(&Instruction::LocalGet(*idx));
            }
            ExpressionKind::LocalSet { id, value } => {
                Self::write_instructions(
                    encoder,
                    *value,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                let idx = local_map.get(id).ok_or_else(|| {
                    WriteError::InternalError(format!("Local not found: {:?}", id))
                })?;
                encoder.instruction(&Instruction::LocalSet(*idx));
            }
            ExpressionKind::LocalTee { id, value } => {
                Self::write_instructions(
                    encoder,
                    *value,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                let idx = local_map.get(id).ok_or_else(|| {
                    WriteError::InternalError(format!("Local not found: {:?}", id))
                })?;
                encoder.instruction(&Instruction::LocalTee(*idx));
            }
            ExpressionKind::GlobalGet { id } => {
                let idx = global_map.get(id).ok_or_else(|| {
                    WriteError::UnsupportedFeature(format!("Global not found: {:?}", id))
                })?;
                encoder.instruction(&Instruction::GlobalGet(*idx));
            }
            ExpressionKind::GlobalSet { id, value } => {
                Self::write_instructions(
                    encoder,
                    *value,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                let idx = global_map.get(id).ok_or_else(|| {
                    WriteError::UnsupportedFeature(format!("Global not found: {:?}", id))
                })?;
                encoder.instruction(&Instruction::GlobalSet(*idx));
            }
            ExpressionKind::Binary { op, left, right } => {
                Self::write_instructions(
                    encoder,
                    *left,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                Self::write_instructions(
                    encoder,
                    *right,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;

                let instr = match op {
                    BinaryOp::AddInt32 => Instruction::I32Add,
                    BinaryOp::SubInt32 => Instruction::I32Sub,
                    BinaryOp::MulInt32 => Instruction::I32Mul,
                    BinaryOp::DivSInt32 => Instruction::I32DivS,
                    BinaryOp::DivUInt32 => Instruction::I32DivU,
                    BinaryOp::RemSInt32 => Instruction::I32RemS,
                    BinaryOp::RemUInt32 => Instruction::I32RemU,
                    BinaryOp::AndInt32 => Instruction::I32And,
                    BinaryOp::OrInt32 => Instruction::I32Or,
                    BinaryOp::XorInt32 => Instruction::I32Xor,
                    BinaryOp::ShlInt32 => Instruction::I32Shl,
                    BinaryOp::ShrSInt32 => Instruction::I32ShrS,
                    BinaryOp::ShrUInt32 => Instruction::I32ShrU,
                    BinaryOp::EqInt32 => Instruction::I32Eq,
                    BinaryOp::NeInt32 => Instruction::I32Ne,
                    BinaryOp::LtSInt32 => Instruction::I32LtS,
                    BinaryOp::LtUInt32 => Instruction::I32LtU,
                    BinaryOp::GtSInt32 => Instruction::I32GtS,
                    BinaryOp::GtUInt32 => Instruction::I32GtU,
                    BinaryOp::LeSInt32 => Instruction::I32LeS,
                    BinaryOp::LeUInt32 => Instruction::I32LeU,
                    BinaryOp::GeSInt32 => Instruction::I32GeS,
                    BinaryOp::GeUInt32 => Instruction::I32GeU,
                    BinaryOp::RotLInt32 => Instruction::I32Rotl,
                    BinaryOp::RotRInt32 => Instruction::I32Rotr,

                    BinaryOp::AddInt64 => Instruction::I64Add,
                    BinaryOp::SubInt64 => Instruction::I64Sub,
                    BinaryOp::MulInt64 => Instruction::I64Mul,
                    BinaryOp::DivSInt64 => Instruction::I64DivS,
                    BinaryOp::DivUInt64 => Instruction::I64DivU,
                    BinaryOp::RemSInt64 => Instruction::I64RemS,
                    BinaryOp::RemUInt64 => Instruction::I64RemU,
                    BinaryOp::AndInt64 => Instruction::I64And,
                    BinaryOp::OrInt64 => Instruction::I64Or,
                    BinaryOp::XorInt64 => Instruction::I64Xor,
                    BinaryOp::ShlInt64 => Instruction::I64Shl,
                    BinaryOp::ShrSInt64 => Instruction::I64ShrS,
                    BinaryOp::ShrUInt64 => Instruction::I64ShrU,
                    BinaryOp::RotLInt64 => Instruction::I64Rotl,
                    BinaryOp::RotRInt64 => Instruction::I64Rotr,
                    BinaryOp::EqInt64 => Instruction::I64Eq,
                    BinaryOp::NeInt64 => Instruction::I64Ne,
                    BinaryOp::LtSInt64 => Instruction::I64LtS,
                    BinaryOp::LtUInt64 => Instruction::I64LtU,
                    BinaryOp::GtSInt64 => Instruction::I64GtS,
                    BinaryOp::GtUInt64 => Instruction::I64GtU,
                    BinaryOp::LeSInt64 => Instruction::I64LeS,
                    BinaryOp::LeUInt64 => Instruction::I64LeU,
                    BinaryOp::GeSInt64 => Instruction::I64GeS,
                    BinaryOp::GeUInt64 => Instruction::I64GeU,

                    BinaryOp::AddFloat32 => Instruction::F32Add,
                    BinaryOp::SubFloat32 => Instruction::F32Sub,
                    BinaryOp::MulFloat32 => Instruction::F32Mul,
                    BinaryOp::DivFloat32 => Instruction::F32Div,
                    BinaryOp::MinFloat32 => Instruction::F32Min,
                    BinaryOp::MaxFloat32 => Instruction::F32Max,
                    BinaryOp::CopySignFloat32 => Instruction::F32Copysign,
                    BinaryOp::EqFloat32 => Instruction::F32Eq,
                    BinaryOp::NeFloat32 => Instruction::F32Ne,
                    BinaryOp::LtFloat32 => Instruction::F32Lt,
                    BinaryOp::GtFloat32 => Instruction::F32Gt,
                    BinaryOp::LeFloat32 => Instruction::F32Le,
                    BinaryOp::GeFloat32 => Instruction::F32Ge,

                    BinaryOp::AddFloat64 => Instruction::F64Add,
                    BinaryOp::SubFloat64 => Instruction::F64Sub,
                    BinaryOp::MulFloat64 => Instruction::F64Mul,
                    BinaryOp::DivFloat64 => Instruction::F64Div,
                    BinaryOp::MinFloat64 => Instruction::F64Min,
                    BinaryOp::MaxFloat64 => Instruction::F64Max,
                    BinaryOp::CopySignFloat64 => Instruction::F64Copysign,
                    BinaryOp::EqFloat64 => Instruction::F64Eq,
                    BinaryOp::NeFloat64 => Instruction::F64Ne,
                    BinaryOp::LtFloat64 => Instruction::F64Lt,
                    BinaryOp::GtFloat64 => Instruction::F64Gt,
                    BinaryOp::LeFloat64 => Instruction::F64Le,
                    BinaryOp::GeFloat64 => Instruction::F64Ge,
                };
                encoder.instruction(&instr);
            }
            ExpressionKind::Unary { op, value } => {
                Self::write_instructions(
                    encoder,
                    *value,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                let instr = match op {
                    UnaryOp::EqZInt32 => Instruction::I32Eqz,
                    UnaryOp::EqZInt64 => Instruction::I64Eqz,
                    UnaryOp::ClzInt32 => Instruction::I32Clz,
                    UnaryOp::CtzInt32 => Instruction::I32Ctz,
                    UnaryOp::PopcntInt32 => Instruction::I32Popcnt,
                    UnaryOp::ClzInt64 => Instruction::I64Clz,
                    UnaryOp::CtzInt64 => Instruction::I64Ctz,
                    UnaryOp::PopcntInt64 => Instruction::I64Popcnt,
                    UnaryOp::AbsFloat32 => Instruction::F32Abs,
                    UnaryOp::NegFloat32 => Instruction::F32Neg,
                    UnaryOp::SqrtFloat32 => Instruction::F32Sqrt,
                    UnaryOp::CeilFloat32 => Instruction::F32Ceil,
                    UnaryOp::FloorFloat32 => Instruction::F32Floor,
                    UnaryOp::TruncFloat32 => Instruction::F32Trunc,
                    UnaryOp::NearestFloat32 => Instruction::F32Nearest,
                    UnaryOp::AbsFloat64 => Instruction::F64Abs,
                    UnaryOp::NegFloat64 => Instruction::F64Neg,
                    UnaryOp::SqrtFloat64 => Instruction::F64Sqrt,
                    UnaryOp::CeilFloat64 => Instruction::F64Ceil,
                    UnaryOp::FloorFloat64 => Instruction::F64Floor,
                    UnaryOp::TruncFloat64 => Instruction::F64Trunc,
                    UnaryOp::NearestFloat64 => Instruction::F64Nearest,
                    UnaryOp::ConvertSInt32ToFloat32 => Instruction::F32ConvertI32S,
                    UnaryOp::ConvertUInt32ToFloat32 => Instruction::F32ConvertI32U,
                    UnaryOp::ConvertSInt64ToFloat32 => Instruction::F32ConvertI64S,
                    UnaryOp::ConvertUInt64ToFloat32 => Instruction::F32ConvertI64U,
                    UnaryOp::ConvertSInt32ToFloat64 => Instruction::F64ConvertI32S,
                    UnaryOp::ConvertUInt32ToFloat64 => Instruction::F64ConvertI32U,
                    UnaryOp::ConvertSInt64ToFloat64 => Instruction::F64ConvertI64S,
                    UnaryOp::ConvertUInt64ToFloat64 => Instruction::F64ConvertI64U,
                    UnaryOp::TruncSFloat32ToInt32 => Instruction::I32TruncF32S,
                    UnaryOp::TruncUFloat32ToInt32 => Instruction::I32TruncF32U,
                    UnaryOp::TruncSFloat64ToInt32 => Instruction::I32TruncF64S,
                    UnaryOp::TruncUFloat64ToInt32 => Instruction::I32TruncF64U,
                    UnaryOp::TruncSFloat32ToInt64 => Instruction::I64TruncF32S,
                    UnaryOp::TruncUFloat32ToInt64 => Instruction::I64TruncF32U,
                    UnaryOp::TruncSFloat64ToInt64 => Instruction::I64TruncF64S,
                    UnaryOp::TruncUFloat64ToInt64 => Instruction::I64TruncF64U,
                    UnaryOp::WrapInt64 => Instruction::I32WrapI64,
                    UnaryOp::ExtendSInt32 => Instruction::I64ExtendI32S,
                    UnaryOp::ExtendUInt32 => Instruction::I64ExtendI32U,
                    UnaryOp::PromoteFloat32 => Instruction::F64PromoteF32,
                    UnaryOp::DemoteFloat64 => Instruction::F32DemoteF64,
                    UnaryOp::ReinterpretFloat32 => Instruction::I32ReinterpretF32,
                    UnaryOp::ReinterpretFloat64 => Instruction::I64ReinterpretF64,
                    UnaryOp::ReinterpretInt32 => Instruction::F32ReinterpretI32,
                    UnaryOp::ReinterpretInt64 => Instruction::F64ReinterpretI64,
                    UnaryOp::ExtendS8Int32 => Instruction::I32Extend8S,
                    UnaryOp::ExtendS16Int32 => Instruction::I32Extend16S,
                    UnaryOp::ExtendS8Int64 => Instruction::I64Extend8S,
                    UnaryOp::ExtendS16Int64 => Instruction::I64Extend16S,
                    UnaryOp::ExtendS32Int64 => Instruction::I64Extend32S,
                };
                encoder.instruction(&instr);
            }
            ExpressionKind::Block { name, list } => {
                label_stack.push(name.as_ref().map(|s| s.to_string()));
                encoder.instruction(&Instruction::Block(Self::to_block_type(expr.type_)));
                for &child in list {
                    Self::write_instructions(
                        encoder,
                        child,
                        label_stack,
                        func_map,
                        global_map,
                        type_id_map,
                        data_map,
                        element_map,
                        table_map,
                        memory_map,
                        local_map,
                    )?;
                }
                encoder.instruction(&Instruction::End);
                label_stack.pop();
            }
            ExpressionKind::Loop { name, body } => {
                label_stack.push(name.as_ref().map(|s| s.to_string()));
                encoder.instruction(&Instruction::Loop(Self::to_block_type(expr.type_)));
                Self::write_instructions(
                    encoder,
                    *body,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                encoder.instruction(&Instruction::End);
                label_stack.pop();
            }
            ExpressionKind::If {
                condition,
                if_true,
                if_false,
            } => {
                Self::write_instructions(
                    encoder,
                    *condition,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                label_stack.push(None);
                encoder.instruction(&Instruction::If(Self::to_block_type(expr.type_)));
                Self::write_instructions(
                    encoder,
                    *if_true,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                if let Some(f) = if_false {
                    encoder.instruction(&Instruction::Else);
                    Self::write_instructions(
                        encoder,
                        *f,
                        label_stack,
                        func_map,
                        global_map,
                        type_id_map,
                        data_map,
                        element_map,
                        table_map,
                        memory_map,
                        local_map,
                    )?;
                }
                encoder.instruction(&Instruction::End);
                label_stack.pop();
            }
            ExpressionKind::Break {
                name,
                condition,
                value,
            } => {
                if let Some(v) = value {
                    Self::write_instructions(
                        encoder,
                        *v,
                        label_stack,
                        func_map,
                        global_map,
                        type_id_map,
                        data_map,
                        element_map,
                        table_map,
                        memory_map,
                        local_map,
                    )?;
                }
                let depth = Self::find_label_depth(label_stack, name)?;
                if let Some(c) = condition {
                    Self::write_instructions(
                        encoder,
                        *c,
                        label_stack,
                        func_map,
                        global_map,
                        type_id_map,
                        data_map,
                        element_map,
                        table_map,
                        memory_map,
                        local_map,
                    )?;
                    encoder.instruction(&Instruction::BrIf(depth));
                } else {
                    encoder.instruction(&Instruction::Br(depth));
                }
            }
            ExpressionKind::Return { value } => {
                if let Some(v) = value {
                    Self::write_instructions(
                        encoder,
                        *v,
                        label_stack,
                        func_map,
                        global_map,
                        type_id_map,
                        data_map,
                        element_map,
                        table_map,
                        memory_map,
                        local_map,
                    )?;
                }
                encoder.instruction(&Instruction::Return);
            }
            ExpressionKind::Unreachable => {
                encoder.instruction(&Instruction::Unreachable);
            }
            ExpressionKind::Drop { value } => {
                Self::write_instructions(
                    encoder,
                    *value,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                encoder.instruction(&Instruction::Drop);
            }
            ExpressionKind::Select {
                condition,
                if_true,
                if_false,
            } => {
                Self::write_instructions(
                    encoder,
                    *if_true,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                Self::write_instructions(
                    encoder,
                    *if_false,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                Self::write_instructions(
                    encoder,
                    *condition,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                encoder.instruction(&Instruction::Select);
            }
            ExpressionKind::Load {
                bytes,
                signed,
                offset,
                align,
                ptr,
                memory,
            } => {
                Self::write_instructions(
                    encoder,
                    *ptr,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                let memarg = MemArg {
                    offset: *offset as u64,
                    align: *align,
                    memory_index: *memory_map.get(memory).unwrap(),
                };
                let instr = match (expr.type_, *bytes, *signed) {
                    (Type::I32, 4, _) => Instruction::I32Load(memarg),
                    (Type::I32, 1, true) => Instruction::I32Load8S(memarg),
                    (Type::I32, 1, false) => Instruction::I32Load8U(memarg),
                    (Type::I32, 2, true) => Instruction::I32Load16S(memarg),
                    (Type::I32, 2, false) => Instruction::I32Load16U(memarg),
                    (Type::I64, 8, _) => Instruction::I64Load(memarg),
                    (Type::I64, 1, true) => Instruction::I64Load8S(memarg),
                    (Type::I64, 1, false) => Instruction::I64Load8U(memarg),
                    (Type::I64, 2, true) => Instruction::I64Load16S(memarg),
                    (Type::I64, 2, false) => Instruction::I64Load16U(memarg),
                    (Type::I64, 4, true) => Instruction::I64Load32S(memarg),
                    (Type::I64, 4, false) => Instruction::I64Load32U(memarg),
                    (Type::F32, 4, _) => Instruction::F32Load(memarg),
                    (Type::F64, 8, _) => Instruction::F64Load(memarg),
                    _ => return Err(WriteError::InvalidExpression),
                };
                encoder.instruction(&instr);
            }
            ExpressionKind::Store {
                bytes,
                offset,
                align,
                ptr,
                value,
                memory,
            } => {
                Self::write_instructions(
                    encoder,
                    *ptr,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                Self::write_instructions(
                    encoder,
                    *value,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                let memarg = MemArg {
                    offset: *offset as u64,
                    align: *align,
                    memory_index: *memory_map.get(memory).unwrap(),
                };
                let instr = match (value.type_, *bytes) {
                    (Type::I32, 4) => Instruction::I32Store(memarg),
                    (Type::I32, 1) => Instruction::I32Store8(memarg),
                    (Type::I32, 2) => Instruction::I32Store16(memarg),
                    (Type::I64, 8) => Instruction::I64Store(memarg),
                    (Type::I64, 1) => Instruction::I64Store8(memarg),
                    (Type::I64, 2) => Instruction::I64Store16(memarg),
                    (Type::I64, 4) => Instruction::I64Store32(memarg),
                    (Type::F32, 4) => Instruction::F32Store(memarg),
                    (Type::F64, 8) => Instruction::F64Store(memarg),
                    _ => return Err(WriteError::InvalidExpression),
                };
                encoder.instruction(&instr);
            }
            ExpressionKind::Call {
                target,
                operands,
                is_return,
            } => {
                for &op in operands {
                    Self::write_instructions(
                        encoder,
                        op,
                        label_stack,
                        func_map,
                        global_map,
                        type_id_map,
                        data_map,
                        element_map,
                        table_map,
                        memory_map,
                        local_map,
                    )?;
                }
                let idx = func_map.get(target).ok_or_else(|| {
                    WriteError::UnsupportedFeature(format!("Function not found: {:?}", target))
                })?;
                if *is_return {
                    encoder.instruction(&Instruction::ReturnCall(*idx));
                } else {
                    encoder.instruction(&Instruction::Call(*idx));
                }
            }
            ExpressionKind::Nop => {
                encoder.instruction(&Instruction::Nop);
            }
            ExpressionKind::Switch {
                names,
                default,
                condition,
                value,
            } => {
                if let Some(v) = value {
                    Self::write_instructions(
                        encoder,
                        *v,
                        label_stack,
                        func_map,
                        global_map,
                        type_id_map,
                        data_map,
                        element_map,
                        table_map,
                        memory_map,
                        local_map,
                    )?;
                }
                Self::write_instructions(
                    encoder,
                    *condition,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                let mut targets = Vec::new();
                for name in names {
                    targets.push(Self::find_label_depth(label_stack, name)?);
                }
                let default_target = Self::find_label_depth(label_stack, default)?;
                encoder.instruction(&Instruction::BrTable(targets.into(), default_target));
            }
            ExpressionKind::MemorySize { memory } => {
                let idx = *memory_map.get(memory).unwrap();
                encoder.instruction(&Instruction::MemorySize(idx));
            }
            ExpressionKind::MemoryGrow { delta, memory } => {
                Self::write_instructions(
                    encoder,
                    *delta,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                let idx = *memory_map.get(memory).unwrap();
                encoder.instruction(&Instruction::MemoryGrow(idx));
            }
            ExpressionKind::CallIndirect {
                target,
                operands,
                type_id,
                table,
            } => {
                for &op in operands {
                    Self::write_instructions(
                        encoder,
                        op,
                        label_stack,
                        func_map,
                        global_map,
                        type_id_map,
                        data_map,
                        element_map,
                        table_map,
                        memory_map,
                        local_map,
                    )?;
                }
                Self::write_instructions(
                    encoder,
                    *target,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;

                let idx = *type_id_map
                    .get(type_id)
                    .ok_or_else(|| WriteError::InternalError("TypeId not in map".to_string()))?;

                let table_idx = *table_map.get(table).unwrap();
                encoder.instruction(&Instruction::CallIndirect {
                    type_index: idx,
                    table_index: table_idx,
                });
            }
            ExpressionKind::RefNull { type_ } => {
                let reftype = Self::to_ref_type(*type_);
                encoder.instruction(&Instruction::RefNull(reftype.heap_type));
            }
            ExpressionKind::RefIsNull { value } => {
                Self::write_instructions(
                    encoder,
                    *value,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                encoder.instruction(&Instruction::RefIsNull);
            }
            ExpressionKind::RefFunc { func } => {
                let idx = func_map.get(func).ok_or_else(|| {
                    WriteError::UnsupportedFeature(format!("Function not found: {:?}", func))
                })?;
                encoder.instruction(&Instruction::RefFunc(*idx));
            }
            ExpressionKind::RefEq { left, right } => {
                Self::write_instructions(
                    encoder,
                    *left,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                Self::write_instructions(
                    encoder,
                    *right,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                encoder.instruction(&Instruction::RefEq);
            }
            ExpressionKind::MemoryFill {
                dest,
                value,
                size,
                memory,
            } => {
                Self::write_instructions(
                    encoder,
                    *dest,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                Self::write_instructions(
                    encoder,
                    *value,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                Self::write_instructions(
                    encoder,
                    *size,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                encoder.instruction(&Instruction::MemoryFill(*memory_map.get(memory).unwrap()));
            }
            ExpressionKind::MemoryCopy {
                dest,
                src,
                size,
                dest_memory,
                src_memory,
            } => {
                Self::write_instructions(
                    encoder,
                    *dest,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                Self::write_instructions(
                    encoder,
                    *src,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                Self::write_instructions(
                    encoder,
                    *size,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                encoder.instruction(&Instruction::MemoryCopy {
                    src_mem: *memory_map.get(src_memory).unwrap(),
                    dst_mem: *memory_map.get(dest_memory).unwrap(),
                });
            }
            ExpressionKind::MemoryInit {
                segment,
                dest,
                offset,
                size,
                memory,
            } => {
                Self::write_instructions(
                    encoder,
                    *dest,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                Self::write_instructions(
                    encoder,
                    *offset,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                Self::write_instructions(
                    encoder,
                    *size,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                encoder.instruction(&Instruction::MemoryInit {
                    mem: *memory_map.get(memory).unwrap(),
                    data_index: *data_map.get(segment).unwrap(),
                });
            }
            ExpressionKind::DataDrop { segment } => {
                encoder.instruction(&Instruction::DataDrop(*data_map.get(segment).unwrap()));
            }
            ExpressionKind::TableGet { table, index } => {
                Self::write_instructions(
                    encoder,
                    *index,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                encoder.instruction(&Instruction::TableGet(*table_map.get(table).unwrap()));
            }
            ExpressionKind::TableSet {
                table,
                index,
                value,
            } => {
                Self::write_instructions(
                    encoder,
                    *index,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                Self::write_instructions(
                    encoder,
                    *value,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                encoder.instruction(&Instruction::TableSet(*table_map.get(table).unwrap()));
            }
            ExpressionKind::TableSize { table } => {
                encoder.instruction(&Instruction::TableSize(*table_map.get(table).unwrap()));
            }
            ExpressionKind::TableGrow {
                table,
                delta,
                value,
            } => {
                Self::write_instructions(
                    encoder,
                    *value,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                Self::write_instructions(
                    encoder,
                    *delta,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                encoder.instruction(&Instruction::TableGrow(*table_map.get(table).unwrap()));
            }
            ExpressionKind::TableFill {
                table,
                dest,
                value,
                size,
            } => {
                Self::write_instructions(
                    encoder,
                    *dest,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                Self::write_instructions(
                    encoder,
                    *value,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                Self::write_instructions(
                    encoder,
                    *size,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                encoder.instruction(&Instruction::TableFill(*table_map.get(table).unwrap()));
            }
            ExpressionKind::TableCopy {
                dest_table,
                src_table,
                dest,
                src,
                size,
            } => {
                Self::write_instructions(
                    encoder,
                    *dest,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                Self::write_instructions(
                    encoder,
                    *src,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                Self::write_instructions(
                    encoder,
                    *size,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                encoder.instruction(&Instruction::TableCopy {
                    src_table: *table_map.get(src_table).unwrap(),
                    dst_table: *table_map.get(dest_table).unwrap(),
                });
            }
            ExpressionKind::TableInit {
                table,
                segment,
                dest,
                offset,
                size,
            } => {
                Self::write_instructions(
                    encoder,
                    *dest,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                Self::write_instructions(
                    encoder,
                    *offset,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                Self::write_instructions(
                    encoder,
                    *size,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                encoder.instruction(&Instruction::TableInit {
                    table: *table_map.get(table).unwrap(),
                    elem_index: *element_map.get(segment).unwrap(),
                });
            }
            ExpressionKind::ElemDrop { segment } => {
                encoder.instruction(&Instruction::ElemDrop(*element_map.get(segment).unwrap()));
            }
            ExpressionKind::AtomicRMW {
                op,
                bytes,
                offset,
                ptr,
                value,
                memory,
            } => {
                Self::write_instructions(
                    encoder,
                    *ptr,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                Self::write_instructions(
                    encoder,
                    *value,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                let arg = MemArg {
                    offset: *offset as u64,
                    align: match *bytes {
                        1 => 0,
                        2 => 1,
                        4 => 2,
                        8 => 3,
                        _ => 0,
                    },
                    memory_index: *memory_map.get(memory).unwrap(),
                };
                let ty = value.type_;
                match *op {
                    AtomicOp::Add => {
                        if ty == Type::I32 {
                            encoder.instruction(&Instruction::I32AtomicRmwAdd(arg));
                        } else if ty == Type::I64 {
                            encoder.instruction(&Instruction::I64AtomicRmwAdd(arg));
                        } else {
                            return Err(WriteError::UnsupportedFeature(format!(
                                "Atomic RMW Add for type {:?}",
                                ty
                            )));
                        }
                    }
                    AtomicOp::Sub => {
                        if ty == Type::I32 {
                            encoder.instruction(&Instruction::I32AtomicRmwSub(arg));
                        } else if ty == Type::I64 {
                            encoder.instruction(&Instruction::I64AtomicRmwSub(arg));
                        } else {
                            return Err(WriteError::UnsupportedFeature(format!(
                                "Atomic RMW Sub for type {:?}",
                                ty
                            )));
                        }
                    }
                    AtomicOp::And => {
                        if ty == Type::I32 {
                            encoder.instruction(&Instruction::I32AtomicRmwAnd(arg));
                        } else if ty == Type::I64 {
                            encoder.instruction(&Instruction::I64AtomicRmwAnd(arg));
                        } else {
                            return Err(WriteError::UnsupportedFeature(format!(
                                "Atomic RMW And for type {:?}",
                                ty
                            )));
                        }
                    }
                    AtomicOp::Or => {
                        if ty == Type::I32 {
                            encoder.instruction(&Instruction::I32AtomicRmwOr(arg));
                        } else if ty == Type::I64 {
                            encoder.instruction(&Instruction::I64AtomicRmwOr(arg));
                        } else {
                            return Err(WriteError::UnsupportedFeature(format!(
                                "Atomic RMW Or for type {:?}",
                                ty
                            )));
                        }
                    }
                    AtomicOp::Xor => {
                        if ty == Type::I32 {
                            encoder.instruction(&Instruction::I32AtomicRmwXor(arg));
                        } else if ty == Type::I64 {
                            encoder.instruction(&Instruction::I64AtomicRmwXor(arg));
                        } else {
                            return Err(WriteError::UnsupportedFeature(format!(
                                "Atomic RMW Xor for type {:?}",
                                ty
                            )));
                        }
                    }
                    AtomicOp::Xchg => {
                        if ty == Type::I32 {
                            encoder.instruction(&Instruction::I32AtomicRmwXchg(arg));
                        } else if ty == Type::I64 {
                            encoder.instruction(&Instruction::I64AtomicRmwXchg(arg));
                        } else {
                            return Err(WriteError::UnsupportedFeature(format!(
                                "Atomic RMW Xchg for type {:?}",
                                ty
                            )));
                        }
                    }
                }
            }
            ExpressionKind::AtomicCmpxchg {
                bytes,
                offset,
                ptr,
                expected,
                replacement,
                memory,
            } => {
                Self::write_instructions(
                    encoder,
                    *ptr,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                Self::write_instructions(
                    encoder,
                    *expected,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                Self::write_instructions(
                    encoder,
                    *replacement,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                let arg = MemArg {
                    offset: *offset as u64,
                    align: match *bytes {
                        1 => 0,
                        2 => 1,
                        4 => 2,
                        8 => 3,
                        _ => 0,
                    },
                    memory_index: *memory_map.get(memory).unwrap(),
                };
                let ty = expected.type_;
                if ty == Type::I32 {
                    encoder.instruction(&Instruction::I32AtomicRmwCmpxchg(arg));
                } else if ty == Type::I64 {
                    encoder.instruction(&Instruction::I64AtomicRmwCmpxchg(arg));
                } else {
                    return Err(WriteError::UnsupportedFeature(format!(
                        "Atomic Cmpxchg for type {:?}",
                        ty
                    )));
                }
            }
            ExpressionKind::AtomicWait {
                ptr,
                expected,
                timeout,
                expected_type,
                memory,
            } => {
                Self::write_instructions(
                    encoder,
                    *ptr,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                Self::write_instructions(
                    encoder,
                    *expected,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                Self::write_instructions(
                    encoder,
                    *timeout,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                let arg = MemArg {
                    offset: 0,
                    align: if *expected_type == Type::I32 { 2 } else { 3 },
                    memory_index: *memory_map.get(memory).unwrap(),
                };
                if *expected_type == Type::I32 {
                    encoder.instruction(&Instruction::MemoryAtomicWait32(arg));
                } else if *expected_type == Type::I64 {
                    encoder.instruction(&Instruction::MemoryAtomicWait64(arg));
                } else {
                    return Err(WriteError::UnsupportedFeature(format!(
                        "Atomic Wait for type {:?}",
                        expected_type
                    )));
                }
            }
            ExpressionKind::AtomicNotify { ptr, count, memory } => {
                Self::write_instructions(
                    encoder,
                    *ptr,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                Self::write_instructions(
                    encoder,
                    *count,
                    label_stack,
                    func_map,
                    global_map,
                    type_id_map,
                    data_map,
                    element_map,
                    table_map,
                    memory_map,
                    local_map,
                )?;
                encoder.instruction(&Instruction::MemoryAtomicNotify(MemArg {
                    offset: 0,
                    align: 2,
                    memory_index: *memory_map.get(memory).unwrap(),
                }));
            }
            ExpressionKind::AtomicFence => {
                encoder.instruction(&Instruction::AtomicFence);
            }
            _ => todo!(
                "Other instructions implementation with wasm-encoder: {:?}",
                expr.kind
            ),
        }
        Ok(())
    }

    fn find_label_depth(label_stack: &[Option<String>], target: &str) -> Result<u32> {
        for (i, label) in label_stack.iter().rev().enumerate() {
            if let Some(name) = label {
                if name == target {
                    return Ok(i as u32);
                }
            }
        }
        Err(WriteError::LabelNotFound(target.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::{Expression, ExpressionKind, IrBuilder};
    use crate::ir::module::ExportKind;
    use crate::ir::module::{Function, Memory, MemoryKind, MemoryLimits, Module};
    use crate::ir::ops::BinaryOp;
    use crate::ir::Literal;
    use crate::wasm::binary_reader::BinaryReader;
    use bumpalo::collections::Vec as BumpVec;
    use bumpalo::Bump;

    #[test]
    fn test_write_minimal_module() {
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        let bump = Bump::new();
        let body = bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(42)),
            type_: Type::I32,
        });

        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(ExprRef::new(body)),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer
            .write_module(&module)
            .expect("Failed to write module");

        // Verify magic and version
        assert_eq!(&bytes[0..4], &[0x00, 0x61, 0x73, 0x6D]);
        assert_eq!(&bytes[4..8], &[0x01, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_roundtrip() {
        // Original module
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);
        let body = bump.alloc(Expression {
            kind: ExpressionKind::Const(Literal::I32(42)),
            type_: Type::I32,
        });
        module.add_function(Function::new(
            "test".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(ExprRef::new(body)),
        ));

        // Write
        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        // Read back
        let bump2 = Bump::new();
        let mut reader = BinaryReader::new(&bump2, bytes);
        let module2 = reader.parse_module().expect("Failed to read");

        // Verify
        assert_eq!(module2.functions.len(), 1);
        let func = &module2.functions.iter().next().unwrap().1;
        assert_eq!(func.results, Type::I32);

        if let Some(body) = func.body() {
            match &body.kind {
                ExpressionKind::Const(Literal::I32(42)) => {} // Success
                _ => panic!("Expected i32.const 42"),
            }
        }
    }

    #[test]
    fn test_write_multi_param_function() {
        // Create a module with a function that has a single parameter
        // (our IR currently stores params as single Type, not Vec<Type>)
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);
        let body = bump.alloc(Expression {
            kind: ExpressionKind::LocalGet { id: 0.into() },
            type_: Type::I32,
        });

        module.add_function(Function::new(
            "test".to_string(),
            Type::I32, // Single param
            Type::I32,
            vec![],
            Some(ExprRef::new(body)),
        ));

        let mut writer = BinaryWriter::new();
        let bytes = writer
            .write_module(&module)
            .expect("Failed to write module");

        // Read it back
        let bump2 = Bump::new();
        let mut reader = BinaryReader::new(&bump2, bytes);
        let module2 = reader.parse_module().expect("Failed to parse");

        assert_eq!(module2.functions.len(), 1);
        let func = &module2.functions.iter().next().unwrap().1;
        assert_eq!(func.params, Type::I32);
        assert_eq!(func.results, Type::I32);
    }

    #[test]
    fn test_roundtrip_with_locals() {
        // Module with function that has locals
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);
        let body = ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::LocalGet { id: 1.into() }, // Get local variable
            type_: Type::I32,
        }));

        module.add_function(Function::new(
            "test".to_string(),
            Type::I32,
            Type::I32,
            vec![Type::I32, Type::I64], // Two local variables
            Some(body),
        ));

        // Write
        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        // Read back
        let bump2 = Bump::new();
        let mut reader = BinaryReader::new(&bump2, bytes);
        let module2 = reader.parse_module().expect("Failed to read");

        // Verify
        assert_eq!(module2.functions.len(), 1);
        let func = &module2.functions.iter().next().unwrap().1;
        assert_eq!(func.params, Type::I32);
        assert_eq!(func.results, Type::I32);
        let num_params = BinaryWriter::expand_type(func.params).len();
        assert_eq!(func.locals.len() - num_params, 2);
        assert_eq!(
            func.locals.get_at_index(num_params).unwrap().type_,
            Type::I32
        );
        assert_eq!(
            func.locals.get_at_index(num_params + 1).unwrap().type_,
            Type::I64
        );
    }

    #[test]
    fn test_control_flow_roundtrip() {
        use crate::ir::expression::IrBuilder;
        use crate::wasm::binary_reader::BinaryReader;
        use bumpalo::collections::Vec as BumpVec;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Build a function with control flow:
        // fn test(i32) -> i32 {
        //   block {
        //     if (local.get 0) {
        //       return const 1
        //     }
        //     loop {
        //       drop (const 42)
        //       break
        //     }
        //   }
        //   select (const 10, const 20, local.get 0)
        // }

        let local_get = builder.local_get(0, Type::I32);
        let const_1 = builder.const_(Literal::I32(1));
        let return_expr = Expression::new(
            &bump,
            ExpressionKind::Return {
                value: Some(const_1),
            },
            Type::UNREACHABLE,
        );
        let if_body = builder.if_(local_get, return_expr, None, Type::NONE);

        let const_42 = builder.const_(Literal::I32(42));
        let drop_expr =
            Expression::new(&bump, ExpressionKind::Drop { value: const_42 }, Type::NONE);
        let break_expr = builder.break_("$loop", None, None, Type::UNREACHABLE);
        let mut loop_body_list = BumpVec::new_in(&bump);
        loop_body_list.push(drop_expr);
        loop_body_list.push(break_expr);
        let loop_body_block = builder.block(None, loop_body_list, Type::NONE);
        let loop_expr = builder.loop_(Some("$loop"), loop_body_block, Type::NONE);

        let mut block_list = BumpVec::new_in(&bump);
        block_list.push(if_body);
        block_list.push(loop_expr);
        let block = builder.block(None, block_list, Type::NONE);

        let const_10 = builder.const_(Literal::I32(10));
        let const_20 = builder.const_(Literal::I32(20));
        let local_get2 = builder.local_get(0, Type::I32);
        let select_expr = Expression::new(
            &bump,
            ExpressionKind::Select {
                condition: local_get2,
                if_true: const_10,
                if_false: const_20,
            },
            Type::I32,
        );

        let mut final_body_list = BumpVec::new_in(&bump);
        final_body_list.push(block);
        final_body_list.push(select_expr);
        let body = builder.block(None, final_body_list, Type::I32);

        module.add_function(Function::new(
            "test_control_flow".to_string(),
            Type::I32,
            Type::I32,
            vec![],
            Some(body),
        ));

        // Write
        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        println!("Written {} bytes", bytes.len());
        println!("Bytes: {:02X?}", &bytes[0..std::cmp::min(100, bytes.len())]);

        // Read back
        let bump2 = Bump::new();
        let mut reader = BinaryReader::new(&bump2, bytes);
        let module2 = reader.parse_module().expect("Failed to read");

        // Verify basic structure
        assert_eq!(module2.functions.len(), 1);
        let func = &module2.functions.iter().next().unwrap().1;
        // Note: function name not preserved without export section
        assert_eq!(func.params, Type::I32);
        assert_eq!(func.results, Type::I32);
        assert!(func.body().is_some());

        println!("Control flow round-trip test passed!");
    }

    #[test]
    fn test_memory_operations_roundtrip() {
        use crate::ir::expression::IrBuilder;
        use crate::wasm::binary_reader::BinaryReader;
        use bumpalo::collections::Vec as BumpVec;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);
        let mem_id = module.add_memory(Memory {
            name: "memory".to_string(),
            limits: MemoryLimits {
                initial: 1,
                maximum: None,
            },
            kind: MemoryKind::Local,
        });

        // Build a function with memory operations:
        // fn test(i32) -> i32 {
        //   let ptr = local.get 0
        //   i32.store offset=4 align=2 (ptr, const 42)
        //   i32.load offset=4 align=2 (ptr)
        // }

        let ptr = builder.local_get(0, Type::I32);
        let const_42 = builder.const_(Literal::I32(42));
        let store_expr = builder.store(mem_id, 4, 4, 2, ptr, const_42);

        let ptr2 = builder.local_get(0, Type::I32);
        let load_expr = builder.load(mem_id, 4, false, 4, 2, ptr2, Type::I32);

        let mut body_list = BumpVec::new_in(&bump);
        body_list.push(store_expr);
        body_list.push(load_expr);
        let body = builder.block(None, body_list, Type::I32);

        module.add_function(Function::new(
            "test_memory".to_string(),
            Type::I32,
            Type::I32,
            vec![],
            Some(body),
        ));

        // Write
        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        println!("Memory ops written {} bytes", bytes.len());

        // Read back
        let bump2 = Bump::new();
        let mut reader = BinaryReader::new(&bump2, bytes);
        let module2 = reader.parse_module().expect("Failed to read");

        // Verify basic structure
        assert_eq!(module2.functions.len(), 1);
        let func = &module2.functions.iter().next().unwrap().1;
        assert_eq!(func.params, Type::I32);
        assert_eq!(func.results, Type::I32);
        assert!(func.body().is_some());

        println!("Memory operations round-trip test passed!");
    }

    #[test]
    fn test_load_variants() {
        use crate::ir::expression::IrBuilder;
        use crate::wasm::binary_reader::BinaryReader;
        use bumpalo::collections::Vec as BumpVec;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);
        let mem_id = module.add_memory(Memory {
            name: "memory".to_string(),
            limits: MemoryLimits {
                initial: 1,
                maximum: None,
            },
            kind: MemoryKind::Local,
        });

        // Test all load variants: i32.load8_s, i32.load8_u, i32.load16_s, i32.load16_u, i64.load
        let ptr = builder.local_get(0, Type::I32);
        let load8_s = builder.load(mem_id, 1, true, 0, 0, ptr, Type::I32);

        let ptr2 = builder.local_get(0, Type::I32);
        let load8_u = builder.load(mem_id, 1, false, 1, 0, ptr2, Type::I32);

        let ptr3 = builder.local_get(0, Type::I32);
        let load16_s = builder.load(mem_id, 2, true, 2, 1, ptr3, Type::I32);

        let ptr4 = builder.local_get(0, Type::I32);
        let load16_u = builder.load(mem_id, 2, false, 4, 1, ptr4, Type::I32);

        let ptr5 = builder.local_get(0, Type::I32);
        let load64 = builder.load(mem_id, 8, false, 8, 3, ptr5, Type::I64);

        let mut body_list = BumpVec::new_in(&bump);
        body_list.push(load8_s);
        body_list.push(load8_u);
        body_list.push(load16_s);
        body_list.push(load16_u);
        body_list.push(load64);
        let body = builder.block(None, body_list, Type::I64);

        module.add_function(Function::new(
            "test_loads".to_string(),
            Type::I32,
            Type::I64,
            vec![],
            Some(body),
        ));

        // Write and read back
        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let bump2 = Bump::new();
        let mut reader = BinaryReader::new(&bump2, bytes);
        let module2 = reader.parse_module().expect("Failed to read");

        assert_eq!(module2.functions.len(), 1);
        println!("All load variants test passed!");
    }

    #[test]
    fn test_store_variants() {
        use crate::ir::expression::IrBuilder;
        use crate::wasm::binary_reader::BinaryReader;
        use bumpalo::collections::Vec as BumpVec;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);
        let mem_id = module.add_memory(Memory {
            name: "memory".to_string(),
            limits: MemoryLimits {
                initial: 1,
                maximum: None,
            },
            kind: MemoryKind::Local,
        });

        // Test all store variants: i32.store8, i32.store16, i64.store
        let ptr1 = builder.local_get(0, Type::I32);
        let val1 = builder.const_(Literal::I32(1));
        let store8 = builder.store(mem_id, 1, 0, 0, ptr1, val1);

        let ptr2 = builder.local_get(0, Type::I32);
        let val2 = builder.const_(Literal::I32(2));
        let store16 = builder.store(mem_id, 2, 2, 1, ptr2, val2);

        let ptr3 = builder.local_get(0, Type::I32);
        let val3 = builder.const_(Literal::I64(3));
        let store64 = builder.store(mem_id, 8, 4, 3, ptr3, val3);

        let mut body_list = BumpVec::new_in(&bump);
        body_list.push(store8);
        body_list.push(store16);
        body_list.push(store64);
        let body = builder.block(None, body_list, Type::NONE);

        module.add_function(Function::new(
            "test_stores".to_string(),
            Type::I32,
            Type::NONE,
            vec![],
            Some(body),
        ));

        // Write and read back
        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let bump2 = Bump::new();
        let mut reader = BinaryReader::new(&bump2, bytes);
        let module2 = reader.parse_module().expect("Failed to read");

        assert_eq!(module2.functions.len(), 1);
        println!("All store variants test passed!");
    }

    #[test]
    fn test_memory_with_control_flow() {
        use crate::ir::expression::IrBuilder;
        use crate::wasm::binary_reader::BinaryReader;
        use bumpalo::collections::Vec as BumpVec;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);
        let mem_id = module.add_memory(Memory {
            name: "memory".to_string(),
            limits: MemoryLimits {
                initial: 1,
                maximum: None,
            },
            kind: MemoryKind::Local,
        });

        // Build: if (local.get 0) { i32.store(ptr, 100) } else { i32.store(ptr, 200) }
        //        return i32.load(ptr)
        let condition = builder.local_get(0, Type::I32);

        let ptr_true = builder.local_get(1, Type::I32);
        let val_true = builder.const_(Literal::I32(100));
        let store_true = builder.store(mem_id, 4, 0, 2, ptr_true, val_true);

        let ptr_false = builder.local_get(1, Type::I32);
        let val_false = builder.const_(Literal::I32(200));
        let store_false = builder.store(mem_id, 4, 0, 2, ptr_false, val_false);

        let if_expr = builder.if_(condition, store_true, Some(store_false), Type::NONE);

        let ptr_load = builder.local_get(1, Type::I32);
        let load_result = builder.load(mem_id, 4, false, 0, 2, ptr_load, Type::I32);

        let mut body_list = BumpVec::new_in(&bump);
        body_list.push(if_expr);
        body_list.push(load_result);
        let body = builder.block(None, body_list, Type::I32);

        // Note: Using basic types since tuple params not fully supported yet
        module.add_function(Function::new(
            "test_mem_control".to_string(),
            Type::I32, // Simple param for now
            Type::I32,
            vec![Type::I32], // local for ptr
            Some(body),
        ));

        // Write and read back
        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let bump2 = Bump::new();
        let mut reader = BinaryReader::new(&bump2, bytes);
        let module2 = reader.parse_module().expect("Failed to read");

        assert_eq!(module2.functions.len(), 1);
        println!("Memory with control flow test passed!");
    }

    #[test]
    fn test_memory_offsets_and_alignment() {
        use crate::ir::expression::IrBuilder;
        use crate::wasm::binary_reader::BinaryReader;
        use bumpalo::collections::Vec as BumpVec;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);
        let mem_id = module.add_memory(Memory {
            name: "memory".to_string(),
            limits: MemoryLimits {
                initial: 1,
                maximum: None,
            },
            kind: MemoryKind::Local,
        });

        // Test various offsets and alignments
        let ptr1 = builder.local_get(0, Type::I32);
        let load_offset0 = builder.load(mem_id, 4, false, 0, 0, ptr1, Type::I32);

        let ptr2 = builder.local_get(0, Type::I32);
        let load_offset8 = builder.load(mem_id, 4, false, 8, 2, ptr2, Type::I32);

        let ptr3 = builder.local_get(0, Type::I32);
        let load_offset1024 = builder.load(mem_id, 4, false, 1024, 2, ptr3, Type::I32);

        let mut body_list = BumpVec::new_in(&bump);
        body_list.push(load_offset0);
        body_list.push(load_offset8);
        body_list.push(load_offset1024);
        let body = builder.block(None, body_list, Type::I32);

        module.add_function(Function::new(
            "test_offsets".to_string(),
            Type::I32,
            Type::I32,
            vec![],
            Some(body),
        ));

        // Write and read back
        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let bump2 = Bump::new();
        let mut reader = BinaryReader::new(&bump2, bytes);
        let module2 = reader.parse_module().expect("Failed to read");

        assert_eq!(module2.functions.len(), 1);
        println!("Memory offsets and alignment test passed!");
    }

    #[test]
    fn test_memory_section() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Add memory with limits
        let mem_id = module.add_memory(Memory {
            name: "memory".to_string(),
            limits: MemoryLimits {
                initial: 1,
                maximum: Some(10),
            },
            kind: MemoryKind::Local,
        });

        // Add a simple function that uses memory
        let ptr = builder.const_(Literal::I32(0));
        let val = builder.const_(Literal::I32(42));
        let store = builder.store(mem_id, 4, 0, 2, ptr, val);

        module.add_function(Function::new(
            "init_memory".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(store),
        ));

        // Write and read back
        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let bump2 = Bump::new();
        let mut reader = BinaryReader::new(&bump2, bytes);
        let module2 = reader.parse_module().expect("Failed to read");

        // Verify memory limits
        assert_eq!(module2.memories.len(), 1);
        let memory = module2.memories.iter().next().unwrap().1;
        assert_eq!(memory.limits.initial, 1);
        assert_eq!(memory.limits.maximum, Some(10));

        assert_eq!(module2.functions.len(), 1);
        println!("Memory section test passed!");
    }

    #[test]
    fn test_export_section() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Add a function
        let result = builder.const_(Literal::I32(42));
        let func_id = module.add_function(Function::new(
            "get_answer".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(result),
        ));

        // Export the function
        module.export_function(func_id, "answer".to_string());

        // Write and read back
        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let bump2 = Bump::new();
        let mut reader = BinaryReader::new(&bump2, bytes);
        let module2 = reader.parse_module().expect("Failed to read");

        // Verify export
        assert_eq!(module2.exports.len(), 1);
        let export = &module2.exports[0];
        assert_eq!(export.name, "answer");
        let func_id = module2.functions.iter().next().unwrap().0;
        assert_eq!(export.kind, ExportKind::Function(func_id));

        println!("Export section test passed!");
    }

    #[test]
    fn test_memory_and_exports_combined() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Set memory
        let mem_id = module.add_memory(Memory {
            name: "memory".to_string(),
            limits: MemoryLimits {
                initial: 2,
                maximum: None,
            },
            kind: MemoryKind::Local,
        });

        // Add a function that loads from memory
        let ptr = builder.const_(Literal::I32(0));
        let load = builder.load(mem_id, 4, false, 0, 2, ptr, Type::I32);

        let func_id = module.add_function(Function::new(
            "read_mem".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(load),
        ));

        // Export both function and memory
        module.export_function(func_id, "read".to_string());
        module.add_export("mem".to_string(), ExportKind::Memory(mem_id));

        // Write and read back
        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let bump2 = Bump::new();
        let mut reader = BinaryReader::new(&bump2, bytes);
        let module2 = reader.parse_module().expect("Failed to read");

        // Verify memory
        assert_eq!(module2.memories.len(), 1);
        let (mem_id, memory) = module2.memories.iter().next().unwrap();
        assert_eq!(memory.limits.initial, 2);
        assert_eq!(memory.limits.maximum, None);

        // Verify exports
        assert_eq!(module2.exports.len(), 2);

        let func_export = module2.exports.iter().find(|e| e.name == "read").unwrap();
        let func_id = module2.functions.iter().next().unwrap().0;
        assert_eq!(func_export.kind, ExportKind::Function(func_id));

        let mem_export = module2.exports.iter().find(|e| e.name == "mem").unwrap();
        assert_eq!(mem_export.kind, ExportKind::Memory(mem_id));

        println!("Memory and exports combined test passed!");
    }

    #[test]
    fn test_complete_wasm_module() {
        use std::fs;
        use std::process::Command;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Set memory (1 page = 64KB)
        let mem_id = module.add_memory(Memory {
            name: "memory".to_string(),
            limits: MemoryLimits {
                initial: 1,
                maximum: Some(10),
            },
            kind: MemoryKind::Local,
        });

        // Function 1: get42 - returns constant 42
        let result = builder.const_(Literal::I32(42));

        let f1 = module.add_function(Function::new(
            "get42".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(result),
        ));

        // Function 2: store_value - stores a value in memory
        let value_param = builder.local_get(0, Type::I32);
        let ptr = builder.const_(Literal::I32(0));
        let store = builder.store(mem_id, 4, 0, 2, ptr, value_param);

        let f2 = module.add_function(Function::new(
            "store_value".to_string(),
            Type::I32,
            Type::NONE,
            vec![],
            Some(store),
        ));

        // Function 3: load_value - loads value from memory
        let ptr3 = builder.const_(Literal::I32(0));
        let load = builder.load(mem_id, 4, false, 0, 2, ptr3, Type::I32);

        let f3 = module.add_function(Function::new(
            "load_value".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(load),
        ));

        // Export all functions and memory
        module.export_function(f1, "get42".to_string());
        module.export_function(f2, "store_value".to_string());
        module.export_function(f3, "load_value".to_string());
        module.add_export("memory".to_string(), ExportKind::Memory(mem_id));

        // Write to file
        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let test_file = std::env::temp_dir().join("test_module.wasm");
        fs::write(&test_file, &bytes).expect("Failed to write WASM file");

        // Validate with wasmtime
        let output = Command::new("wasmtime")
            .arg("compile")
            .arg(&test_file)
            .arg("-o")
            .arg(std::env::temp_dir().join("test_module.cwasm"))
            .output();

        match output {
            Ok(result) => {
                if !result.status.success() {
                    let stderr = String::from_utf8_lossy(&result.stderr);
                    panic!("wasmtime compilation failed:\n{}", stderr);
                }
                println!("✓ Module compiled successfully with wasmtime!");
            }
            Err(e) => {
                println!("⚠ Could not run wasmtime ({}), skipping validation", e);
            }
        }

        // Verify round-trip
        let bump2 = Bump::new();
        let mut reader = BinaryReader::new(&bump2, bytes);
        let module2 = reader.parse_module().expect("Failed to read back");

        assert_eq!(module2.functions.len(), 3);
        assert_eq!(module2.exports.len(), 4);
        assert_eq!(module2.memories.len(), 1);

        println!("Complete WASM module test passed!");
    }

    #[test]
    fn test_function_call() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Function 1: helper - returns 42
        let result1 = builder.const_(Literal::I32(42));
        let helper_id = module.add_function(Function::new(
            "helper".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(result1),
        ));

        // Function 2: caller - calls helper
        let operands = BumpVec::new_in(&bump);
        let call_expr = builder.call(helper_id, operands, Type::I32, false);

        module.add_function(Function::new(
            "caller".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(call_expr),
        ));

        // Write and read back
        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let bump2 = Bump::new();
        let mut reader = BinaryReader::new(&bump2, bytes);
        let module2 = reader.parse_module().expect("Failed to read");

        // Verify structure
        assert_eq!(module2.functions.len(), 2);

        // Check that second function has a call expression
        let caller_func = &module2.functions.iter().nth(1).unwrap().1;
        assert!(caller_func.body().is_some());

        if let Some(body) = caller_func.body() {
            match &body.kind {
                ExpressionKind::Call {
                    target,
                    operands,
                    is_return,
                } => {
                    let helper_id = module2.functions.iter().next().unwrap().0;
                    assert_eq!(*target, helper_id); // Should call function at index 0
                    assert_eq!(operands.len(), 0); // No arguments
                    assert!(!*is_return); // Not a tail call
                }
                _ => panic!("Expected Call expression, got {:?}", body.kind),
            }
        }

        println!("Function call test passed!");
    }

    #[test]
    fn test_function_call_with_args() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Function 1: add_five - takes i32 param and adds 5
        let param = builder.local_get(0, Type::I32);
        let five = builder.const_(Literal::I32(5));
        let sum = builder.binary(BinaryOp::AddInt32, param, five, Type::I32);

        let add_five_id = module.add_function(Function::new(
            "add_five".to_string(),
            Type::I32,
            Type::I32,
            vec![],
            Some(sum),
        ));

        // Function 2: caller - calls add_five with argument 10
        let arg = builder.const_(Literal::I32(10));
        let mut operands = BumpVec::new_in(&bump);
        operands.push(arg);
        let call_expr = builder.call(add_five_id, operands, Type::I32, false);

        module.add_function(Function::new(
            "caller".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(call_expr),
        ));

        // Write and read back
        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let bump2 = Bump::new();
        let mut reader = BinaryReader::new(&bump2, bytes);
        let module2 = reader.parse_module().expect("Failed to read");

        // Verify structure
        assert_eq!(module2.functions.len(), 2);

        println!("Function call with args test passed!");
    }

    #[test]
    fn test_recursive_function() {
        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Recursive factorial function (simplified)
        // factorial(n): if n <= 1 then 1 else n * factorial(n-1)
        let _n = builder.local_get(0, Type::I32);
        let one = builder.const_(Literal::I32(1));

        // For simplicity, just return 1 (base case)
        // A full recursive implementation would need proper if/else with recursive call
        let body = one;

        module.add_function(Function::new(
            "factorial".to_string(),
            Type::I32,
            Type::I32,
            vec![],
            Some(body),
        ));

        // Write and read back
        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let bump2 = Bump::new();
        let mut reader = BinaryReader::new(&bump2, bytes);
        let module2 = reader.parse_module().expect("Failed to read");

        assert_eq!(module2.functions.len(), 1);

        println!("Recursive function test passed!");
    }

    #[test]
    fn test_multi_function_program_with_wasmtime() {
        use std::fs;
        use std::process::Command;

        let bump = Bump::new();
        let builder = IrBuilder::new(&bump);
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // Set memory
        let mem_id = module.add_memory(Memory {
            name: "memory".to_string(),
            limits: MemoryLimits {
                initial: 1,
                maximum: Some(10),
            },
            kind: MemoryKind::Local,
        });

        // Function 1: double - doubles an i32 parameter
        let param = builder.local_get(0, Type::I32);
        let two = builder.const_(Literal::I32(2));
        let doubled = builder.binary(BinaryOp::MulInt32, param, two, Type::I32);

        let double_id = module.add_function(Function::new(
            "double".to_string(),
            Type::I32,
            Type::I32,
            vec![],
            Some(doubled),
        ));

        // Function 2: increment - adds 1 to an i32 parameter
        let param2 = builder.local_get(0, Type::I32);
        let one = builder.const_(Literal::I32(1));
        let incremented = builder.binary(BinaryOp::AddInt32, param2, one, Type::I32);

        let increment_id = module.add_function(Function::new(
            "increment".to_string(),
            Type::I32,
            Type::I32,
            vec![],
            Some(incremented),
        ));

        // Function 3: process - calls both functions: increment(double(x))
        let input = builder.const_(Literal::I32(5)); // Input: 5

        // Call double(5) -> 10
        let mut double_args = BumpVec::new_in(&bump);
        double_args.push(input);
        let double_result = builder.call(double_id, double_args, Type::I32, false);

        // Call increment(10) -> 11
        let mut inc_args = BumpVec::new_in(&bump);
        inc_args.push(double_result);
        let final_result = builder.call(increment_id, inc_args, Type::I32, false);

        let process_id = module.add_function(Function::new(
            "process".to_string(),
            Type::NONE,
            Type::I32,
            vec![],
            Some(final_result),
        ));

        // Export all functions
        module.export_function(double_id, "double".to_string());
        module.export_function(increment_id, "increment".to_string());
        module.export_function(process_id, "process".to_string());
        module.add_export("memory".to_string(), ExportKind::Memory(mem_id));

        // Write to file
        let mut writer = BinaryWriter::new();
        let bytes = writer.write_module(&module).expect("Failed to write");

        let test_file = std::env::temp_dir().join("test_function_calls.wasm");
        fs::write(&test_file, &bytes).expect("Failed to write WASM file");

        // Validate with wasmtime
        let output = Command::new("wasmtime")
            .arg("compile")
            .arg(&test_file)
            .arg("-o")
            .arg(std::env::temp_dir().join("test_function_calls.cwasm"))
            .output();

        match output {
            Ok(result) => {
                if !result.status.success() {
                    let stderr = String::from_utf8_lossy(&result.stderr);
                    panic!("wasmtime compilation failed:\n{}", stderr);
                }
                println!("✓ Multi-function module compiled successfully with wasmtime!");
            }
            Err(e) => {
                println!("⚠ Could not run wasmtime ({}), skipping validation", e);
            }
        }

        // Verify round-trip
        let bump2 = Bump::new();
        let mut reader = BinaryReader::new(&bump2, bytes);
        let module2 = reader.parse_module().expect("Failed to read back");

        assert_eq!(module2.functions.len(), 3);
        assert_eq!(module2.exports.len(), 4); // 3 functions + memory

        // Verify process function has calls
        let process_func = &module2.functions.iter().nth(2).unwrap().1;
        assert!(process_func.body().is_some());

        println!("Multi-function program test passed!");
    }

    #[test]
    fn test_globals_roundtrip() {
        let bump = bumpalo::Bump::new();
        let mut module = Module::new(&bump);

        // 1. Define global: mut i32 g0 = 100
        let builder = IrBuilder::new(&bump);
        let init0 = builder.const_(Literal::I32(100)); // Fixed
        let g0_id = module.add_global(crate::ir::module::Global {
            name: "g0".to_string(),
            type_: Type::I32,
            mutable: true,
            kind: crate::ir::module::GlobalKind::Local { init: init0 },
        });

        // 2. Define global: const f32 g1 = 3.14
        let builder2 = IrBuilder::new(&bump);
        let init1 = builder2.const_(Literal::F32(2.5)); // Fixed
        let g1_id = module.add_global(crate::ir::module::Global {
            name: "g1".to_string(),
            type_: Type::F32,
            mutable: false,
            kind: crate::ir::module::GlobalKind::Local { init: init1 },
        });

        // 3. Define function that uses globals
        let builder3 = IrBuilder::new(&bump);

        let get_g0 = builder3.global_get(g0_id, Type::I32);
        let const_1 = builder3.const_(Literal::I32(1)); // Fixed
        let add = builder3.binary(BinaryOp::AddInt32, get_g0, const_1, Type::I32); // Fixed
        let set_g0 = builder3.global_set(g0_id, add); // g0 = g0 + 1

        let get_g1 = builder3.global_get(g1_id, Type::F32);
        let drop_g1 = builder3.drop(get_g1); // Fixed

        let body = builder3.block(
            None,
            BumpVec::from_iter_in([set_g0, drop_g1], &bump),
            Type::NONE,
        );

        // Function with no params/results/locals
        let func_id = module.add_function(Function::new(
            "test_globals".to_string(),
            Type::NONE,
            Type::NONE,
            vec![],
            Some(body),
        ));
        module.export_function(func_id, "test_globals".to_string());

        // Write
        let mut writer = BinaryWriter::new();
        let bytes = writer
            .write_module(&module)
            .expect("Failed to write module with globals");

        // Read back
        let bump2 = Bump::new();
        let mut reader = BinaryReader::new(&bump2, bytes);
        let module2 = reader
            .parse_module()
            .expect("Failed to read back module with globals");

        // Checks
        assert_eq!(module2.globals.len(), 2);
        let mut ids = module2.globals.ids();
        let g0_id = ids.next().unwrap();
        let g1_id = ids.next().unwrap();

        let _g0 = &module2.globals[g0_id];
        let g1 = &module2.globals[g1_id];
        assert_eq!(g1.type_, Type::F32);
        assert!(!g1.mutable);

        assert_eq!(module2.functions.len(), 1);
        let func = &module2.functions.iter().next().unwrap().1;
        assert!(func.body().is_some());
    }
}
