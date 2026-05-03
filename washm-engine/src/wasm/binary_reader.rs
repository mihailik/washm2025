use crate::collections::id_arena::Id;
use crate::ir::expression::{Expression, ExpressionId, ExpressionKind, IrBuilder};
use crate::ir::module::{
    DataSegment, ElementSegment, Export, ExportKind, Function, MemoryLimits, Module, TableLimits,
};
use crate::ir::{HeapType, Literal, Type};
use gimli::{EndianSlice, LittleEndian};
use std::io;
use wasmparser::{
    Name, NameSectionReader, Operator, OperatorsReader, Parser, Payload, TypeRef, ValType,
};

pub struct BinaryReader {
    types: Vec<(Vec<Type>, Vec<Type>)>,
    all_function_types: Vec<u32>,
    next_label_id: u32,
    last_op_was_else: bool,
    import_names: Vec<String>,
    func_ids: Vec<crate::ir::FuncId>,
    table_ids: Vec<crate::ir::TableId>,
    memory_ids: Vec<crate::ir::MemoryId>,
    global_ids: Vec<crate::ir::GlobalId>,
    type_ids: Vec<crate::ir::TypeId>,
    data_ids: Vec<crate::ir::DataId>,
    element_ids: Vec<crate::ir::ElementId>,
    current_func_local_ids: Vec<crate::ir::LocalId>,
    current_func_local_types: Vec<crate::ir::Type>,
    current_func_body_start: Option<usize>,
    body_starts: Vec<u64>,
    expr_offsets: std::collections::HashMap<ExpressionId, u32>,
    expr_to_func: std::collections::HashMap<ExpressionId, usize>,
    current_function_index: Option<usize>,
}

#[derive(Debug)]
pub enum ParseError {
    UnexpectedEof,
    InvalidMagic,
    InvalidVersion,
    InvalidSection,
    InvalidOpcode(u32),
    InvalidType,
    InvalidUtf8,
    InvalidExportKind,
    InvalidImportKind,
    InvalidIndex(&'static str, u32),
    Io(io::Error),
    Wasmparser(wasmparser::BinaryReaderError),
    LabelNotFound(String),
}

impl From<io::Error> for ParseError {
    fn from(e: io::Error) -> Self {
        ParseError::Io(e)
    }
}

impl From<wasmparser::BinaryReaderError> for ParseError {
    fn from(e: wasmparser::BinaryReaderError) -> Self {
        ParseError::Wasmparser(e)
    }
}

type Result<T> = std::result::Result<T, ParseError>;

impl Default for BinaryReader {
    fn default() -> Self {
        Self::new()
    }
}

impl BinaryReader {
    pub fn new() -> Self {
        Self {
            types: Vec::new(),
            all_function_types: Vec::new(),
            next_label_id: 0,
            last_op_was_else: false,
            import_names: Vec::new(),
            func_ids: Vec::new(),
            table_ids: Vec::new(),
            memory_ids: Vec::new(),
            global_ids: Vec::new(),
            type_ids: Vec::new(),
            data_ids: Vec::new(),
            element_ids: Vec::new(),
            current_func_local_ids: Vec::new(),
            current_func_local_types: Vec::new(),
            current_func_body_start: None,
            body_starts: Vec::new(),
            expr_offsets: std::collections::HashMap::new(),
            expr_to_func: std::collections::HashMap::new(),
            current_function_index: None,
        }
    }

    fn make_label_name(&mut self) -> String {
        let name = format!("L_{}", self.next_label_id);
        self.next_label_id += 1;
        name
    }

    fn record<T>(&mut self, id: Id<T>, offset: u32) -> Id<T> {
        let rel_off = if let Some(start) = self.current_func_body_start {
            if offset >= start as u32 {
                offset - start as u32
            } else {
                offset
            }
        } else {
            offset
        };
        // Use 1-based offsets so that recorded expression offsets are never zero.
        // This avoids ambiguous zero offsets and matches writer expectations.
        let one_based = rel_off.saturating_add(1u32);
        self.expr_offsets.insert(id.cast(), one_based);
        if let Some(idx) = self.current_function_index {
            self.expr_to_func.insert(id.cast(), idx);
        }
        id
    }

    fn valtype_to_type(ty: ValType) -> Type {
        match ty {
            ValType::I32 => Type::I32,
            ValType::I64 => Type::I64,
            ValType::F32 => Type::F32,
            ValType::F64 => Type::F64,
            ValType::V128 => Type::V128,
            ValType::Ref(rt) => {
                let nullable = rt.is_nullable();
                match rt.heap_type() {
                    wasmparser::HeapType::Abstract { shared: _, ty } => match ty {
                        wasmparser::AbstractHeapType::Func => {
                            if nullable {
                                Type::FUNCREF
                            } else {
                                Type::new(HeapType::FUNC, false)
                            }
                        }
                        wasmparser::AbstractHeapType::Extern => {
                            if nullable {
                                Type::EXTERNREF
                            } else {
                                Type::new(HeapType::EXT, false)
                            }
                        }
                        wasmparser::AbstractHeapType::Any => {
                            if nullable {
                                Type::ANYREF
                            } else {
                                Type::new(HeapType::ANY, false)
                            }
                        }
                        wasmparser::AbstractHeapType::Eq => {
                            if nullable {
                                Type::EQREF
                            } else {
                                Type::new(HeapType::EQ, false)
                            }
                        }
                        wasmparser::AbstractHeapType::I31 => {
                            if nullable {
                                Type::I31REF
                            } else {
                                Type::new(HeapType::I31, false)
                            }
                        }
                        wasmparser::AbstractHeapType::Struct => {
                            if nullable {
                                Type::STRUCTREF
                            } else {
                                Type::new(HeapType::STRUCT, false)
                            }
                        }
                        wasmparser::AbstractHeapType::Array => {
                            if nullable {
                                Type::ARRAYREF
                            } else {
                                Type::new(HeapType::ARRAY, false)
                            }
                        }
                        _ => Type::NONE,
                    },
                    _ => Type::NONE,
                }
            }
        }
    }

    fn blocktype_to_type(&self, bt: wasmparser::BlockType) -> Type {
        match bt {
            wasmparser::BlockType::Empty => Type::NONE,
            wasmparser::BlockType::Type(vt) => Self::valtype_to_type(vt),
            wasmparser::BlockType::FuncType(idx) => {
                if let Some(ty) = self.types.get(idx as usize) {
                    Self::types_to_type(&ty.1)
                } else {
                    Type::NONE
                }
            }
        }
    }

    fn types_to_type(types: &[Type]) -> Type {
        match types.len() {
            0 => Type::NONE,
            1 => types[0],
            _ => crate::ir::type_store::intern_tuple(types.to_vec()),
        }
    }

    pub fn parse_module(&mut self, data: &[u8]) -> Result<Module> {
        self.expr_offsets.clear();
        self.expr_to_func.clear();
        self.body_starts.clear();
        let mut module = Module::new();
        let mut function_section = Vec::new();
        let mut import_count = 0;
        let mut code_entry_count = 0;

        for payload in Parser::new(0).parse_all(data) {
            match payload? {
                Payload::TypeSection(reader) => {
                    for rec_group in reader {
                        let rec_group = rec_group?;
                        for sub_type in rec_group.types() {
                            if let wasmparser::CompositeInnerType::Func(func_type) =
                                &sub_type.composite_type.inner
                            {
                                let params: Vec<Type> = func_type
                                    .params()
                                    .iter()
                                    .map(|&t| Self::valtype_to_type(t))
                                    .collect();
                                let results: Vec<Type> = func_type
                                    .results()
                                    .iter()
                                    .map(|&t| Self::valtype_to_type(t))
                                    .collect();

                                let params_type = Self::types_to_type(&params);
                                let results_type = Self::types_to_type(&results);
                                let id = module.add_type(params_type, results_type);
                                self.types.push((params, results));
                                self.type_ids.push(id);
                            }
                        }
                    }
                }
                Payload::ImportSection(reader) => {
                    for imports in reader {
                        let imports = imports?;
                        let (module_name, field_name, ty) = match imports {
                            wasmparser::Imports::Single(_, import) => {
                                (import.module, import.name, import.ty)
                            }
                            wasmparser::Imports::Compact1 { module, items } => {
                                let item = items.into_iter().next().unwrap()?; // This is not quite correct for Compact, but let's follow the previous logic which seemed to handle it
                                (module, item.name, item.ty)
                            }
                            wasmparser::Imports::Compact2 { module, ty, names } => {
                                let name = names.into_iter().next().unwrap()?;
                                (module, name, ty)
                            }
                        };

                        match ty {
                            TypeRef::Func(type_idx) => {
                                let func_type = self
                                    .types
                                    .get(type_idx as usize)
                                    .ok_or(ParseError::InvalidType)?;
                                let type_id = self
                                    .type_ids
                                    .get(type_idx as usize)
                                    .copied()
                                    .ok_or(ParseError::InvalidType)?;

                                let params = Self::types_to_type(&func_type.0);
                                let results = Self::types_to_type(&func_type.1);

                                let mut func = crate::ir::module::Function::new_import(
                                    String::new(),
                                    module_name.to_string(),
                                    field_name.to_string(),
                                    params,
                                    results,
                                );
                                func.type_idx = Some(type_id);

                                let id = module.add_function(func);
                                self.func_ids.push(id);
                                import_count += 1;

                                self.all_function_types.push(type_idx);
                                self.import_names.push(field_name.to_string());
                            }
                            TypeRef::Table(table_type) => {
                                let limits = crate::ir::module::TableLimits {
                                    element_type: Self::valtype_to_type(ValType::Ref(
                                        table_type.element_type,
                                    )),
                                    initial: table_type.initial as u32,
                                    maximum: table_type.maximum.map(|m| m as u32),
                                };
                                let id = module.add_table(crate::ir::module::Table {
                                    name: String::new(),
                                    limits,
                                    kind: crate::ir::module::TableKind::Import {
                                        module: module_name.to_string(),
                                        field: field_name.to_string(),
                                    },
                                });
                                self.table_ids.push(id);
                            }
                            TypeRef::Memory(memory_type) => {
                                let limits = crate::ir::module::MemoryLimits {
                                    initial: memory_type.initial as u32,
                                    maximum: memory_type.maximum.map(|m| m as u32),
                                };
                                let id = module.add_memory(crate::ir::module::Memory {
                                    name: String::new(),
                                    limits,
                                    kind: crate::ir::module::MemoryKind::Import {
                                        module: module_name.to_string(),
                                        field: field_name.to_string(),
                                    },
                                });
                                self.memory_ids.push(id);
                            }
                            TypeRef::Global(global_type) => {
                                let ty = Self::valtype_to_type(global_type.content_type);
                                let mutable = global_type.mutable;
                                let id = module.add_global(crate::ir::module::Global {
                                    name: String::new(),
                                    type_: ty,
                                    mutable,
                                    kind: crate::ir::module::GlobalKind::Import {
                                        module: module_name.to_string(),
                                        field: field_name.to_string(),
                                    },
                                });
                                self.global_ids.push(id);
                            }
                            _ => {}
                        }
                    }
                }
                Payload::FunctionSection(reader) => {
                    for type_idx in reader {
                        let type_idx = type_idx?;
                        function_section.push(type_idx);
                        self.all_function_types.push(type_idx);

                        let func_type = self
                            .types
                            .get(type_idx as usize)
                            .ok_or(ParseError::InvalidType)?;
                        let params = Self::types_to_type(&func_type.0);
                        let results = Self::types_to_type(&func_type.1);

                        let mut locals = crate::collections::id_arena::IdArena::new();
                        let mut param_ids = Vec::new();
                        for t in params.tuple_elements() {
                            param_ids.push(
                                locals
                                    .alloc(crate::ir::module::Local {
                                        name: String::new(),
                                        type_: t,
                                        h_type: None,
                                    })
                                    .cast(),
                            );
                        }

                        let id = module.add_function(Function {
                            name: String::new(),
                            params,
                            results,
                            type_idx: Some(self.type_ids[type_idx as usize]),
                            kind: crate::ir::module::FunctionKind::Local {
                                locals,
                                param_ids,
                                var_ids: Vec::new(),
                                body: ExpressionId::null(), // Placeholder
                            },
                        });
                        self.func_ids.push(id);
                    }
                }
                Payload::TableSection(reader) => {
                    for table in reader {
                        let table = table?;
                        let id = module.add_table(crate::ir::module::Table {
                            name: String::new(),
                            limits: TableLimits {
                                element_type: Self::valtype_to_type(ValType::Ref(
                                    table.ty.element_type,
                                )),
                                initial: table.ty.initial as u32,
                                maximum: table.ty.maximum.map(|m| m as u32),
                            },
                            kind: crate::ir::module::TableKind::Local,
                        });
                        self.table_ids.push(id);
                    }
                }
                Payload::MemorySection(reader) => {
                    for memory in reader {
                        let memory = memory?;
                        let id = module.add_memory(crate::ir::module::Memory {
                            name: String::new(),
                            limits: MemoryLimits {
                                initial: memory.initial as u32,
                                maximum: memory.maximum.map(|m| m as u32),
                            },
                            kind: crate::ir::module::MemoryKind::Local,
                        });
                        self.memory_ids.push(id);
                    }
                }
                Payload::GlobalSection(reader) => {
                    for global in reader {
                        let global = global?;
                        let type_ = Self::valtype_to_type(global.ty.content_type);
                        let mutable = global.ty.mutable;

                        let init = self.parse_expression(
                            global.init_expr.get_operators_reader(),
                            &mut module,
                        )?;
                        let name = String::new();

                        let id = module.add_global(crate::ir::module::Global {
                            name,
                            type_,
                            mutable,
                            kind: crate::ir::module::GlobalKind::Local {
                                init: init.ok_or(ParseError::UnexpectedEof)?,
                            },
                        });
                        self.global_ids.push(id);
                    }
                }
                Payload::ExportSection(reader) => {
                    for export in reader {
                        let export = export?;
                        let kind = match export.kind {
                            wasmparser::ExternalKind::Func => {
                                let id =
                                    self.func_ids.get(export.index as usize).copied().ok_or(
                                        ParseError::InvalidIndex("func export", export.index),
                                    )?;
                                // Propagate export name to function name if missing
                                if let Some(func) = module.functions.get_mut(id) {
                                    // Prefer export names over generic `func_X` names so the
                                    // decompiler prints the real, meaningful function names.
                                    if func.name.is_empty() || func.name.starts_with("func_") {
                                        func.name = export.name.to_string();
                                    }
                                }
                                ExportKind::Function(id)
                            }
                            wasmparser::ExternalKind::Table => {
                                let id = self.table_ids.get(export.index as usize).copied().ok_or(
                                    ParseError::InvalidIndex("table export", export.index),
                                )?;
                                ExportKind::Table(id)
                            }
                            wasmparser::ExternalKind::Memory => {
                                let id =
                                    self.memory_ids.get(export.index as usize).copied().ok_or(
                                        ParseError::InvalidIndex("memory export", export.index),
                                    )?;
                                ExportKind::Memory(id)
                            }
                            wasmparser::ExternalKind::Global => {
                                let id =
                                    self.global_ids.get(export.index as usize).copied().ok_or(
                                        ParseError::InvalidIndex("global export", export.index),
                                    )?;
                                ExportKind::Global(id)
                            }
                            _ => continue,
                        };
                        module.exports.push(Export {
                            name: export.name.to_string(),
                            kind,
                        });
                    }
                }
                Payload::StartSection { func, .. } => {
                    let id = self
                        .func_ids
                        .get(func as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("start func", func))?;
                    module.set_start(id);
                }
                Payload::ElementSection(reader) => {
                    for element in reader {
                        let element = element?;
                        let offset = match element.kind {
                            wasmparser::ElementKind::Active {
                                ref offset_expr, ..
                            } => self.parse_expression(
                                offset_expr.get_operators_reader(),
                                &mut module,
                            )?,
                            _ => None,
                        };

                        let mut func_indices = Vec::new();
                        if let wasmparser::ElementItems::Functions(funcs) = element.items {
                            for f in funcs {
                                let f_idx = f?;
                                let id = self
                                    .func_ids
                                    .get(f_idx as usize)
                                    .copied()
                                    .ok_or(ParseError::InvalidIndex("element func", f_idx))?;
                                func_indices.push(id);
                            }
                        }

                        let table_index = match element.kind {
                            wasmparser::ElementKind::Active { table_index, .. } => {
                                let idx = table_index.unwrap_or(0);
                                self.table_ids
                                    .get(idx as usize)
                                    .copied()
                                    .ok_or(ParseError::InvalidIndex("element table", idx))?
                            }
                            _ => return Err(ParseError::InvalidSection), // Passive/Declarative not handled yet
                        };

                        let id = module.add_element_segment(ElementSegment {
                            table_index,
                            offset: offset.ok_or(ParseError::UnexpectedEof)?,
                            func_indices,
                        });
                        self.element_ids.push(id);
                    }
                }
                Payload::CodeSectionEntry(body) => {
                    let start = body.range().start;
                    self.body_starts.push(start as u64);
                    let func_id = self.func_ids[import_count + code_entry_count];

                    // DEBUG: Check name
                    // if let Some(f) = module.functions.get(func_id) {
                    //     eprintln!("DEBUG: CodeSectionEntry for {:?}, current name: '{}'", func_id, f.name);
                    // }

                    // Record the current function index (global index including imports)
                    self.current_function_index = Some(import_count + code_entry_count);
                    // Record the start of the function body (including size LEB) to allow relative offsets
                    // consistent with DWARF absolute offsets in the Code section.
                    self.current_func_body_start = Some(start);
                    code_entry_count += 1;

                    // 1. First, get the locals and update the local_ids mapping
                    let (var_types, mapping, full_types) = {
                        let func = module.get_function(func_id).unwrap();
                        let mut var_types = Vec::new();
                        let mut reader = body.get_locals_reader()?;
                        for _ in 0..reader.get_count() {
                            let (count, ty) = reader.read()?;
                            let t = Self::valtype_to_type(ty);
                            for _ in 0..count {
                                var_types.push(t);
                            }
                        }

                        let mut mapping = Vec::new();
                        let mut full_types = Vec::new();

                        // FIX: We need the actual LocalIds for parameters.
                        match &func.kind {
                            crate::ir::module::FunctionKind::Local { param_ids, .. } => {
                                mapping.extend_from_slice(param_ids);
                                full_types.extend(func.params.tuple_elements());
                            }
                            crate::ir::module::FunctionKind::Import(_) => {
                                // This shouldn't happen for CodeSectionEntry
                            }
                        }

                        full_types.extend_from_slice(&var_types);

                        (var_types, mapping, full_types)
                    };

                    // Add locals to function and complete the mapping
                    let mut mapping = mapping;
                    {
                        let func = module.get_function_mut(func_id).unwrap();
                        for t in var_types {
                            let id = func.add_var(t);
                            mapping.push(id);
                        }
                    }
                    self.current_func_local_ids = mapping;
                    self.current_func_local_types = full_types;

                    // 2. Now parse the expression with mutable access to module
                    let expr = self.parse_expression(body.get_operators_reader()?, &mut module)?;

                    // 3. Finally set the body
                    let func = module.get_function_mut(func_id).unwrap();
                    if let Some(mut_body) = func.body_mut() {
                        if let Some(e) = expr {
                            *mut_body = e;
                        }
                    }
                    // Clear current function index now that we've parsed the body
                    self.current_function_index = None;
                    self.current_func_body_start = None;
                }
                Payload::DataSection(reader) => {
                    for data in reader {
                        let data = data?;
                        let offset = match data.kind {
                            wasmparser::DataKind::Active {
                                ref offset_expr, ..
                            } => self.parse_expression(
                                offset_expr.get_operators_reader(),
                                &mut module,
                            )?,
                            _ => None,
                        };

                        let memory_index = match data.kind {
                            wasmparser::DataKind::Active { memory_index, .. } => self
                                .memory_ids
                                .get(memory_index as usize)
                                .copied()
                                .ok_or(ParseError::InvalidIndex("data memory", memory_index))?,
                            _ => return Err(ParseError::InvalidSection),
                        };

                        let id = module.add_data_segment(DataSegment {
                            memory_index,
                            offset: offset.ok_or(ParseError::UnexpectedEof)?,
                            data: data.data.to_vec(),
                        });
                        self.data_ids.push(id);
                    }
                }
                Payload::CustomSection(reader) => {
                    let name = reader.name();
                    if name == "name" {
                        let name_reader = NameSectionReader::new(wasmparser::BinaryReader::new(
                            reader.data(),
                            reader.data_offset(),
                        ));
                        for subsection in name_reader.flatten() {
                            match subsection {
                                Name::Function(map) => {
                                    for naming in map.into_iter().flatten() {
                                        if let Some(&id) = self.func_ids.get(naming.index as usize)
                                        {
                                            if let Some(f) = module.functions.get_mut(id) {
                                                let new_name = naming.name.to_string();
                                                if f.name.is_empty() || f.name.starts_with("func_")
                                                {
                                                    f.name = new_name;
                                                }
                                            }
                                        }
                                    }
                                }
                                Name::Global(map) => {
                                    for naming in map.into_iter().flatten() {
                                        if let Some(&id) =
                                            self.global_ids.get(naming.index as usize)
                                        {
                                            if let Some(g) = module.globals.get_mut(id) {
                                                g.name = naming.name.to_string();
                                            }
                                        }
                                    }
                                }
                                Name::Table(map) => {
                                    for naming in map.into_iter().flatten() {
                                        if let Some(&id) = self.table_ids.get(naming.index as usize)
                                        {
                                            if let Some(t) = module.tables.get_mut(id) {
                                                t.name = naming.name.to_string();
                                            }
                                        }
                                    }
                                }
                                Name::Memory(map) => {
                                    for naming in map.into_iter().flatten() {
                                        if let Some(&id) =
                                            self.memory_ids.get(naming.index as usize)
                                        {
                                            if let Some(m) = module.memories.get_mut(id) {
                                                m.name = naming.name.to_string();
                                            }
                                        }
                                    }
                                }
                                Name::Local(map) => {
                                    for naming in map.into_iter().flatten() {
                                        if let Some(&id) = self.func_ids.get(naming.index as usize)
                                        {
                                            if let Some(f) = module.functions.get_mut(id) {
                                                if let crate::ir::module::FunctionKind::Local {
                                                    locals,
                                                    ..
                                                } = &mut f.kind
                                                {
                                                    for l in naming.names.into_iter().flatten() {
                                                        let l_id =
                                                            crate::ir::LocalId::from_u32(l.index);
                                                        if let Some(local) = locals.get_mut(l_id) {
                                                            local.name = l.name.to_string();
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    module
                        .custom_sections
                        .push(crate::ir::module::CustomSection {
                            name: name.to_string(),
                            data: reader.data().to_vec(),
                        });
                }
                _ => {}
            }
        }

        self.apply_dwarf(&mut module, data);
        Ok(module)
    }

    fn apply_dwarf(&self, module: &mut Module, _data: &[u8]) {
        let num_imports = module
            .functions
            .iter()
            .filter(|(_, f)| f.is_import())
            .count();

        // Build mapping of absolute offsets for expressions
        log::debug!(
            "apply_dwarf: expr_offsets.len={} expr_to_func.len={} body_starts={:?}",
            self.expr_offsets.len(),
            self.expr_to_func.len(),
            self.body_starts
        );
        let mut func_to_offsets: std::collections::HashMap<usize, Vec<(u32, ExpressionId)>> =
            std::collections::HashMap::new();
        for (&expr_id, &off) in &self.expr_offsets {
            log::debug!(
                "apply_dwarf: expr {:?} off {} func_to {:?}",
                expr_id,
                off,
                self.expr_to_func.get(&expr_id)
            );
            if let Some(&func_idx) = self.expr_to_func.get(&expr_id) {
                if func_idx < num_imports {
                    continue;
                }
                let local_func_idx = func_idx - num_imports;
                let body_start = self.body_starts.get(local_func_idx).copied().unwrap_or(0) as u32;

                // All recorded offsets are relative to the start of the function entry (at the LEB)
                let abs_off = body_start + off;
                func_to_offsets
                    .entry(func_idx)
                    .or_default()
                    .push((abs_off, expr_id));
            }
        }
        for offsets in func_to_offsets.values_mut() {
            offsets.sort_by_key(|&(off, _)| off);
        }
        for (func_idx, offsets) in &func_to_offsets {
            log::debug!("func_to_offsets[{}] => {:?}", func_idx, offsets);
        }

        if let Some(s) = module
            .custom_sections
            .iter()
            .find(|s| s.name == ".debug_line")
        {
            log::debug!(
                "Found .debug_line section ({} bytes), attempting full DWARF parse",
                s.data.len()
            );

            let body_starts = &self.body_starts;
            let mut applied = 0usize;

            // (line_program_offset, local_file_index) -> global_file_index
            let mut global_file_map: std::collections::HashMap<(u64, u64), u32> =
                std::collections::HashMap::new();

            let mut func_to_offsets: std::collections::HashMap<usize, Vec<(u32, ExpressionId)>> =
                std::collections::HashMap::new();
            for (&expr_id, &off) in &self.expr_offsets {
                if let Some(&func_idx) = self.expr_to_func.get(&expr_id) {
                    if func_idx < num_imports {
                        continue;
                    }
                    let local_func_idx = func_idx - num_imports;
                    let body_start = body_starts.get(local_func_idx).copied().unwrap_or(0) as u32;

                    // All recorded offsets are now relative to the start of local_decls (body_start)
                    let abs_off = body_start + off;
                    func_to_offsets
                        .entry(func_idx)
                        .or_default()
                        .push((abs_off, expr_id));
                }
            }
            for offsets in func_to_offsets.values_mut() {
                offsets.sort_by_key(|&(off, _)| off);
            }

            // Build a Dwarf object from available sections in module.custom_sections.
            use gimli::SectionId;

            let mut section_map: std::collections::HashMap<&str, Vec<u8>> =
                std::collections::HashMap::new();
            // Prefer the first-seen section for a given name (writer-generated sections are
            // appended before original compiled DWARF sections) so we inspect writer-emitted
            // DWARF instead of being overwritten by pre-existing DWARF data from compiled
            // input modules.
            for sec in &module.custom_sections {
                section_map
                    .entry(sec.name.as_str())
                    .or_insert_with(|| sec.data.clone());
            }

            // Loader closure: supply EndianSlice readers for sections we have; otherwise empty slice.
            let loader = |id: SectionId| -> gimli::read::Result<EndianSlice<LittleEndian>> {
                let name = id.name();
                let data = section_map.get(name).map_or(&[][..], |v| v.as_slice());
                Ok(gimli::EndianSlice::new(data, LittleEndian))
            };

            match gimli::read::Dwarf::load(loader) {
                Ok(dwarf) => {
                    let mut units = dwarf.units();
                    let mut unit_count = 0;
                    while let Ok(Some(header)) = units.next() {
                        unit_count += 1;
                        if let Ok(unit) = dwarf.unit(header) {
                            log::debug!(
                                "Processing DWARF unit {}, addr_size={}",
                                unit_count,
                                unit.header.address_size()
                            );
                            let addr_size = unit.header.address_size();
                            let mut entries = unit.entries();
                            while let Ok(Some((_depth, entry))) = entries.next_dfs() {
                                if let Ok(Some(attr)) = entry.attr(gimli::DW_AT_stmt_list) {
                                    if let gimli::AttributeValue::DebugLineRef(line_off) =
                                        attr.value()
                                    {
                                        let lp_off = line_off.0 as u64;
                                        if let Ok(program) = dwarf
                                            .debug_line
                                            .program(line_off, addr_size, None, None)
                                        {
                                            let mut rows = program.rows();
                                            // Collect (address, global_idx, line) tuples from the program rows
                                            let mut rows_vec =
                                                Vec::<(u64, u32, Option<u32>)>::new();
                                            while let Ok(Some((header, row))) = rows.next_row() {
                                                if row.end_sequence() {
                                                    continue;
                                                }
                                                let address = row.address();
                                                if address == 0 {
                                                    continue;
                                                }

                                                // Capture or resolve the global filename index
                                                let local_idx_raw = row.file_index();
                                                if local_idx_raw == 0 {
                                                    continue;
                                                }

                                                let global_idx = *global_file_map
                                                    .entry((lp_off, local_idx_raw))
                                                    .or_insert_with(|| {
                                                        let new_idx =
                                                            module.debug_filenames.len() as u32;
                                                        let mut name_found = String::new();
                                                        if let Some(file_entry) = header.file(local_idx_raw) {
                                                            let mut path = String::new();
                                                            let dir_idx = file_entry.directory_index();
                                                            if dir_idx > 0 {
                                                                if let Some(dir_attr) =
                                                                    header.directory(dir_idx)
                                                                {
                                                                    if let Ok(dir_name) = dwarf
                                                                        .attr_string(
                                                                            &unit, dir_attr,
                                                                        )
                                                                    {
                                                                        path.push_str(
                                                                            &dir_name
                                                                                .to_string_lossy(),
                                                                        );
                                                                        path.push('/');
                                                                    }
                                                                }
                                                            }
                                                            if let Ok(name) = dwarf.attr_string(
                                                                &unit,
                                                                file_entry.path_name(),
                                                            ) {
                                                                path.push_str(
                                                                    &name.to_string_lossy(),
                                                                );
                                                            }
                                                            name_found = path;
                                                        }
                                                        if name_found.is_empty() {
                                                            name_found = format!("file{}", local_idx_raw);
                                                        }
                                                        log::debug!("DWARF: mapped (lp_off={}, local_idx={}) -> global_idx={} ({})", lp_off, local_idx_raw, new_idx, name_found);
                                                        module.debug_filenames.push(name_found);
                                                        new_idx
                                                    });

                                                let line_num = row.line().map(|n| n.get() as u32);

                                                rows_vec.push((address, global_idx, line_num));
                                            }
                                            log::debug!(
                                                "Collected {} rows from line program",
                                                rows_vec.len()
                                            );
                                            for (address, global_idx, line_num) in rows_vec {
                                                let addr32 = address as u32;
                                                // Find which function this address belongs to using body_starts
                                                log::debug!(
                                                    "Processing row: addr={} global_idx={}",
                                                    address,
                                                    global_idx
                                                );
                                                let local_func_idx =
                                                    match body_starts.binary_search(&{ address }) {
                                                        Ok(idx) => idx,
                                                        Err(idx) => {
                                                            if idx > 0 {
                                                                idx - 1
                                                            } else {
                                                                continue;
                                                            }
                                                        }
                                                    };
                                                let func_idx = local_func_idx + num_imports;
                                                log::debug!(
                                                    "Row belongs to local_func_idx={} func_idx={}",
                                                    local_func_idx,
                                                    func_idx
                                                );

                                                if let Some(offsets) =
                                                    func_to_offsets.get(&func_idx)
                                                {
                                                    log::debug!(
                                                        "Offsets for func {}: {:?}",
                                                        func_idx,
                                                        offsets
                                                    );
                                                    // Find nearest-lower matching expression offset using binary search
                                                    let best_idx = match offsets
                                                        .binary_search_by_key(&addr32, |&(o, _)| o)
                                                    {
                                                        Ok(idx) => {
                                                            log::debug!(
                                                                "Exact match at idx {}",
                                                                idx
                                                            );
                                                            Some(idx)
                                                        }
                                                        Err(idx) => {
                                                            log::debug!(
                                                                "binary_search insertion idx {}",
                                                                idx
                                                            );
                                                            if idx > 0 {
                                                                Some(idx - 1)
                                                            } else {
                                                                // Fallback: if the first recorded expression offset is exactly one
                                                                // greater than the row address (off-by-one discrepancy due to
                                                                // one-based vs zero-based offsets), treat it as a match.
                                                                if !offsets.is_empty() {
                                                                    let (first_off, _eid) =
                                                                        offsets[0];
                                                                    if first_off == addr32 + 1 {
                                                                        log::debug!("Applying fallback match: first_off == addr+1 ({} == {})", first_off, addr32 + 1);
                                                                        Some(0)
                                                                    } else {
                                                                        None
                                                                    }
                                                                } else {
                                                                    None
                                                                }
                                                            }
                                                        }
                                                    };

                                                    if let Some(idx) = best_idx {
                                                        let (abs_off, expr_id) = offsets[idx];
                                                        log::debug!(
                                                            "Best match abs_off={} expr_id={:?}",
                                                            abs_off,
                                                            expr_id
                                                        );

                                                        if addr32 >= abs_off
                                                            || abs_off == addr32.saturating_add(1)
                                                        {
                                                            let line = line_num;

                                                            module.annotations.set_location(
                                                                expr_id,
                                                                crate::ir::annotation::DebugLocation {
                                                                    file_index: global_idx,
                                                                    line: line.unwrap_or(0u32),
                                                                    column: 0u32,
                                                                },
                                                            );
                                                            applied += 1;
                                                        }
                                                    }
                                                } else {
                                                    log::debug!("No offsets for func {}", func_idx);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    log::info!("Applied {} debug_line annotations via full DWARF", applied);
                }
                Err(e) => {
                    log::warn!("Failed to construct gimli Dwarf: {:?}", e);
                }
            }
        }
    }

    /// Expose the expression offset map for testing.
    pub fn expr_offsets_map(&self) -> &std::collections::HashMap<ExpressionId, u32> {
        &self.expr_offsets
    }

    fn parse_expression(
        &mut self,
        mut reader: OperatorsReader,
        module: &mut Module,
    ) -> Result<Option<ExpressionId>> {
        self.parse_expression_impl(&mut reader, &mut Vec::new(), module)
    }

    fn parse_expression_impl(
        &mut self,
        reader: &mut OperatorsReader,
        label_stack: &mut Vec<Option<String>>,
        module: &mut Module,
    ) -> Result<Option<ExpressionId>> {
        struct InstructionStack {
            scopes: Vec<Vec<ExpressionId>>,
        }

        impl InstructionStack {
            fn new() -> Self {
                Self {
                    scopes: vec![Vec::new()],
                }
            }

            fn push(&mut self, expr: ExpressionId) {
                self.scopes.last_mut().unwrap().push(expr);
            }

            fn enter_scope(&mut self) {
                self.scopes.push(Vec::new());
            }

            fn exit_scope(&mut self) -> Vec<ExpressionId> {
                self.scopes.pop().expect("Balanced scopes")
            }

            fn into_inner(mut self) -> Vec<ExpressionId> {
                self.scopes.pop().expect("Final scope")
            }
        }

        let mut stack = InstructionStack::new();
        let mut value_stack = Vec::new();
        let mut consumed = Box::new(std::collections::HashSet::new());

        // Iterative control frames to avoid recursion (blocks/loops/ifs).
        #[derive(Debug)]
        enum FrameKind {
            Block,
            Loop,
            If {
                condition: ExpressionId,
                if_true: Option<ExpressionId>,
                seen_else: bool,
            },
        }
        struct Frame {
            kind: FrameKind,
            name: String,
            expression_id: ExpressionId,
            result_type: Type,
            _start_value_stack_len: usize,
            start_offset: u32,
        }
        let mut frames: Vec<Frame> = Vec::new();

        // Note: we implement finalization inline where needed to avoid borrow conflicts.

        while !reader.eof() {
            let offset = reader.original_position() as u32;
            let op = reader.read()?;
            match op {
                Operator::End => {
                    // If no open control frames, End means end of this expression scope.
                    if frames.is_empty() {
                        self.last_op_was_else = false;
                        break;
                    }

                    // Close the topmost frame.
                    let frame = frames.pop().unwrap();
                    // finalize the expressions that belong to this frame
                    let body_expr = {
                        let final_stack: Vec<ExpressionId> = stack
                            .exit_scope()
                            .into_iter()
                            .filter(|e| !consumed.contains(e))
                            .collect();
                        if final_stack.is_empty() {
                            self.record(
                                IrBuilder::new(&mut module.expressions).nop(),
                                frame.start_offset,
                            )
                        } else if final_stack.len() > 1 {
                            let result_type = final_stack
                                .last()
                                .map(|&e| module.expressions[e].type_)
                                .unwrap_or(Type::NONE);
                            self.record(
                                IrBuilder::new(&mut module.expressions).block(
                                    Some(String::new()),
                                    final_stack,
                                    result_type,
                                ),
                                frame.start_offset,
                            )
                        } else {
                            final_stack[0]
                        }
                    };

                    match frame.kind {
                        FrameKind::Block => {
                            label_stack.pop();
                            module.expressions[frame.expression_id].kind = ExpressionKind::Block {
                                name: Some(frame.name),
                                list: vec![body_expr],
                            };
                            module.expressions[frame.expression_id].type_ = frame.result_type;
                            let block_expr = self.record(frame.expression_id, frame.start_offset);
                            stack.push(block_expr);
                            if frame.result_type != Type::NONE {
                                value_stack.push(block_expr);
                            }
                        }
                        FrameKind::Loop => {
                            label_stack.pop();
                            module.expressions[frame.expression_id].kind = ExpressionKind::Loop {
                                name: Some(frame.name),
                                body: body_expr,
                            };
                            module.expressions[frame.expression_id].type_ = frame.result_type;
                            let loop_expr = self.record(frame.expression_id, frame.start_offset);
                            stack.push(loop_expr);
                            if frame.result_type != Type::NONE {
                                value_stack.push(loop_expr);
                            }
                        }
                        FrameKind::If {
                            condition,
                            if_true,
                            seen_else,
                        } => {
                            label_stack.pop();
                            let if_expr_final = if !seen_else {
                                self.record(
                                    module.expressions.alloc(Expression {
                                        type_: frame.result_type,
                                        kind: ExpressionKind::If {
                                            name: None,
                                            condition,
                                            if_true: body_expr,
                                            if_false: None,
                                        },
                                    }),
                                    frame.start_offset,
                                )
                            } else {
                                let if_true_expr = if_true.expect("if_true must be present");
                                self.record(
                                    module.expressions.alloc(Expression {
                                        type_: frame.result_type,
                                        kind: ExpressionKind::If {
                                            name: None,
                                            condition,
                                            if_true: if_true_expr,
                                            if_false: Some(body_expr),
                                        },
                                    }),
                                    frame.start_offset,
                                )
                            };

                            module.expressions[frame.expression_id].kind = ExpressionKind::Block {
                                name: Some(frame.name),
                                list: vec![if_expr_final],
                            };
                            module.expressions[frame.expression_id].type_ = frame.result_type;
                            let if_wrapper = self.record(frame.expression_id, frame.start_offset);
                            stack.push(if_wrapper);
                            if frame.result_type != Type::NONE {
                                value_stack.push(if_wrapper);
                            }
                        }
                    }
                }
                Operator::Else => {
                    // If Else occurs at top-level (no enclosing If frame to handle it), behave like before.
                    if frames.is_empty() {
                        self.last_op_was_else = true;
                        break;
                    }

                    // Only valid inner Else if the topmost frame is an If.
                    if let Some(top) = frames.last_mut() {
                        match &mut top.kind {
                            FrameKind::If {
                                condition: _,
                                if_true,
                                seen_else,
                            } => {
                                // finalize the then-branch which is the current accumulated expressions
                                let then_expr = {
                                    let final_stack: Vec<ExpressionId> = stack
                                        .exit_scope()
                                        .into_iter()
                                        .filter(|e| !consumed.contains(e))
                                        .collect();
                                    if final_stack.is_empty() {
                                        self.record(
                                            IrBuilder::new(&mut module.expressions).nop(),
                                            top.start_offset,
                                        )
                                    } else if final_stack.len() > 1 {
                                        let result_type = final_stack
                                            .last()
                                            .map(|&e| module.expressions[e].type_)
                                            .unwrap_or(Type::NONE);
                                        self.record(
                                            IrBuilder::new(&mut module.expressions).block(
                                                Some(String::new()),
                                                final_stack,
                                                result_type,
                                            ),
                                            top.start_offset,
                                        )
                                    } else {
                                        final_stack[0]
                                    }
                                };
                                *if_true = Some(then_expr);
                                *seen_else = true;
                                stack.enter_scope(); // Start a new scope for the else branch
                                                     // indicate an else was seen
                                self.last_op_was_else = true;
                                continue; // continue parsing else branch
                            }
                            _ => {
                                // Else not associated with an If; preserve old behavior
                                self.last_op_was_else = true;
                                break;
                            }
                        }
                    }
                }
                Operator::Unreachable => {
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unreachable(),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::Nop => {
                    let expr = self.record(IrBuilder::new(&mut module.expressions).nop(), offset);
                    stack.push(expr);
                }
                Operator::Block { blockty } => {
                    let result_type = self.blocktype_to_type(blockty);
                    let name = self.make_label_name();
                    let expression_id = module.expressions.alloc(Expression {
                        type_: result_type,
                        kind: ExpressionKind::Nop,
                    });
                    label_stack.push(Some(name.clone()));
                    stack.enter_scope();
                    frames.push(Frame {
                        kind: FrameKind::Block,
                        name,
                        expression_id,
                        result_type,
                        _start_value_stack_len: value_stack.len(),
                        start_offset: offset,
                    });
                }
                Operator::Loop { blockty } => {
                    let result_type = self.blocktype_to_type(blockty);
                    let name = self.make_label_name();
                    let expression_id = module.expressions.alloc(Expression {
                        type_: result_type,
                        kind: ExpressionKind::Nop,
                    });
                    label_stack.push(Some(name.clone()));
                    stack.enter_scope();
                    frames.push(Frame {
                        kind: FrameKind::Loop,
                        name,
                        expression_id,
                        result_type,
                        _start_value_stack_len: value_stack.len(),
                        start_offset: offset,
                    });
                }
                Operator::If { blockty } => {
                    let result_type = self.blocktype_to_type(blockty);
                    let condition = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(condition);
                    let name = self.make_label_name();
                    let expression_id = module.expressions.alloc(Expression {
                        type_: result_type,
                        kind: ExpressionKind::Nop,
                    });
                    label_stack.push(Some(name.clone()));
                    stack.enter_scope();
                    frames.push(Frame {
                        kind: FrameKind::If {
                            condition,
                            if_true: None,
                            seen_else: false,
                        },
                        name,
                        expression_id,
                        result_type,
                        _start_value_stack_len: value_stack.len(),
                        start_offset: offset,
                    });
                }
                Operator::Br { relative_depth } => {
                    let depth = relative_depth as usize;
                    let target = if depth < frames.len() {
                        frames[frames.len() - 1 - depth].expression_id
                    } else {
                        panic!("Invalid break depth");
                    };

                    let value = value_stack.pop();
                    if let Some(v) = value {
                        consumed.insert(v);
                    }
                    let break_expr = self.record(
                        IrBuilder::new(&mut module.expressions).break_(
                            target,
                            None,
                            value,
                            Type::UNREACHABLE,
                        ),
                        offset,
                    );
                    stack.push(break_expr);
                    value_stack.push(break_expr);
                }
                Operator::BrIf { relative_depth } => {
                    let depth = relative_depth as usize;
                    let target = if depth < frames.len() {
                        frames[frames.len() - 1 - depth].expression_id
                    } else {
                        panic!("Invalid break depth");
                    };

                    let condition = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(condition);
                    let value = value_stack.pop();
                    let ty = if let Some(v) = value {
                        consumed.insert(v);
                        module.expressions[v].type_
                    } else {
                        Type::NONE
                    };
                    let break_expr = self.record(
                        IrBuilder::new(&mut module.expressions).break_(
                            target,
                            Some(condition),
                            value,
                            ty,
                        ),
                        offset,
                    );
                    stack.push(break_expr);
                    if ty != Type::NONE {
                        value_stack.push(break_expr);
                    }
                }
                Operator::BrTable { targets } => {
                    let mut target_ids = Vec::with_capacity(targets.len() as usize);
                    for depth in targets.targets() {
                        let depth = depth?;
                        let target = if (depth as usize) < frames.len() {
                            frames[frames.len() - 1 - (depth as usize)].expression_id
                        } else {
                            panic!("Invalid branch depth");
                        };
                        target_ids.push(target);
                    }

                    let default_depth = targets.default();
                    let default_target = if (default_depth as usize) < frames.len() {
                        frames[frames.len() - 1 - (default_depth as usize)].expression_id
                    } else {
                        panic!("Invalid branch depth");
                    };

                    let condition = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(condition);
                    let value = if !value_stack.is_empty() {
                        let v = value_stack.pop().unwrap();
                        consumed.insert(v);
                        Some(v)
                    } else {
                        None
                    };

                    let switch_expr = self.record(
                        IrBuilder::new(&mut module.expressions).switch(
                            target_ids,
                            default_target,
                            condition,
                            value,
                        ),
                        offset,
                    );
                    stack.push(switch_expr);
                    value_stack.push(switch_expr);
                }
                Operator::Return => {
                    let value = value_stack.pop();
                    if let Some(v) = value {
                        consumed.insert(v);
                    }
                    let return_expr = self.record(
                        IrBuilder::new(&mut module.expressions).return_(value),
                        offset,
                    );
                    stack.push(return_expr);
                    value_stack.push(return_expr);
                }
                Operator::Drop => {
                    let value = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(value);
                    let drop_expr =
                        self.record(IrBuilder::new(&mut module.expressions).drop(value), offset);
                    stack.push(drop_expr);
                }
                Operator::Select => {
                    let condition = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(condition);
                    let if_false = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(if_false);
                    let if_true = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(if_true);
                    let result_type = module.expressions[if_true].type_;
                    let select_expr = self.record(
                        IrBuilder::new(&mut module.expressions).select(
                            condition,
                            if_true,
                            if_false,
                            result_type,
                        ),
                        offset,
                    );
                    stack.push(select_expr);
                    value_stack.push(select_expr);
                }
                Operator::Call { function_index } | Operator::ReturnCall { function_index } => {
                    let is_return = matches!(op, Operator::ReturnCall { .. });
                    let type_idx = self
                        .all_function_types
                        .get(function_index as usize)
                        .ok_or(ParseError::InvalidOpcode(0x10))?;
                    let func_type = self
                        .types
                        .get(*type_idx as usize)
                        .ok_or(ParseError::InvalidType)?;

                    let param_count = func_type.0.len();
                    let mut operands = Vec::with_capacity(param_count);
                    for _ in 0..param_count {
                        let op = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                        consumed.insert(op);
                        operands.push(op);
                    }
                    operands.reverse();

                    let func_id = self
                        .func_ids
                        .get(function_index as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("call func", function_index))?;

                    let result_type = Self::types_to_type(&func_type.1);

                    let call_expr = self.record(
                        IrBuilder::new(&mut module.expressions).call(
                            func_id,
                            operands,
                            result_type,
                            is_return,
                        ),
                        offset,
                    );
                    stack.push(call_expr);
                    if result_type != Type::NONE {
                        value_stack.push(call_expr);
                    }
                }
                Operator::CallIndirect {
                    type_index,
                    table_index,
                }
                | Operator::ReturnCallIndirect {
                    type_index,
                    table_index,
                } => {
                    let is_return = matches!(op, Operator::ReturnCallIndirect { .. });
                    let func_type = self
                        .types
                        .get(type_index as usize)
                        .ok_or(ParseError::InvalidType)?;
                    let type_id = self
                        .type_ids
                        .get(type_index as usize)
                        .copied()
                        .ok_or(ParseError::InvalidType)?;
                    let param_count = func_type.0.len();

                    let result_type = Self::types_to_type(&func_type.1);

                    let target = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(target);
                    let mut operands = Vec::with_capacity(param_count);
                    for _ in 0..param_count {
                        let op = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                        consumed.insert(op);
                        operands.push(op);
                    }
                    operands.reverse();

                    let table_id = self
                        .table_ids
                        .get(table_index as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("call_indirect table", table_index))?;

                    let call_expr = self.record(
                        IrBuilder::new(&mut module.expressions).call_indirect(
                            table_id,
                            target,
                            operands,
                            type_id,
                            result_type,
                            is_return,
                        ),
                        offset,
                    );
                    stack.push(call_expr);
                    if result_type != Type::NONE {
                        value_stack.push(call_expr);
                    }
                }
                Operator::LocalGet { local_index } => {
                    let id = self
                        .current_func_local_ids
                        .get(local_index as usize)
                        .copied()
                        .unwrap_or_else(|| {
                            println!(
                                "WARN: Invalid local.get index {} (len {}), using 0",
                                local_index,
                                self.current_func_local_ids.len()
                            );
                            self.current_func_local_ids
                                .first()
                                .copied()
                                .unwrap_or(crate::ir::module::LocalId::null())
                        });
                    /* .ok_or(ParseError::InvalidIndex("local.get", local_index))?; */
                    let ty = self
                        .current_func_local_types
                        .get(local_index as usize)
                        .copied()
                        .unwrap_or(Type::I32);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).local_get(id, ty),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::LocalSet { local_index } => {
                    let id = self
                        .current_func_local_ids
                        .get(local_index as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("local.set", local_index))?;
                    let value = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(value);
                    stack.push(self.record(
                        IrBuilder::new(&mut module.expressions).local_set(id, value),
                        offset,
                    ));
                }
                Operator::LocalTee { local_index } => {
                    let id = self
                        .current_func_local_ids
                        .get(local_index as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("local.tee", local_index))?;
                    let value = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(value);
                    let ty = self
                        .current_func_local_types
                        .get(local_index as usize)
                        .copied()
                        .unwrap_or(Type::I32);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).local_tee(id, value, ty),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::GlobalGet { global_index } => {
                    let id = self
                        .global_ids
                        .get(global_index as usize)
                        .copied()
                        .ok_or(ParseError::InvalidSection)?;
                    let ty = module.globals[id].type_;
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).global_get(id, ty),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::GlobalSet { global_index } => {
                    let id = self
                        .global_ids
                        .get(global_index as usize)
                        .copied()
                        .ok_or(ParseError::InvalidSection)?;
                    let value = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(value);
                    stack.push(self.record(
                        IrBuilder::new(&mut module.expressions).global_set(id, value),
                        offset,
                    ));
                }
                Operator::I32Load { memarg } => {
                    let ptr = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(ptr);
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).load(
                            memory,
                            4,
                            false,
                            memarg.offset as u32,
                            memarg.align as u32,
                            ptr,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64Load { memarg } => {
                    let ptr = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(ptr);
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).load(
                            memory,
                            8,
                            false,
                            memarg.offset as u32,
                            memarg.align as u32,
                            ptr,
                            Type::I64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F32Load { memarg } => {
                    let ptr = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(ptr);
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).load(
                            memory,
                            4,
                            false,
                            memarg.offset as u32,
                            memarg.align as u32,
                            ptr,
                            Type::F32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F64Load { memarg } => {
                    let ptr = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(ptr);
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).load(
                            memory,
                            8,
                            false,
                            memarg.offset as u32,
                            memarg.align as u32,
                            ptr,
                            Type::F64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32Load8S { memarg } => {
                    let ptr = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(ptr);
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).load(
                            memory,
                            1,
                            true,
                            memarg.offset as u32,
                            memarg.align as u32,
                            ptr,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32Load8U { memarg } => {
                    let ptr = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(ptr);
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).load(
                            memory,
                            1,
                            false,
                            memarg.offset as u32,
                            memarg.align as u32,
                            ptr,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32Load16S { memarg } => {
                    let ptr = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(ptr);
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).load(
                            memory,
                            2,
                            true,
                            memarg.offset as u32,
                            memarg.align as u32,
                            ptr,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32Load16U { memarg } => {
                    let ptr = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(ptr);
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).load(
                            memory,
                            2,
                            false,
                            memarg.offset as u32,
                            memarg.align as u32,
                            ptr,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64Load8S { memarg } => {
                    let ptr = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(ptr);
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).load(
                            memory,
                            1,
                            true,
                            memarg.offset as u32,
                            memarg.align as u32,
                            ptr,
                            Type::I64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64Load8U { memarg } => {
                    let ptr = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(ptr);
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).load(
                            memory,
                            1,
                            false,
                            memarg.offset as u32,
                            memarg.align as u32,
                            ptr,
                            Type::I64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64Load16S { memarg } => {
                    let ptr = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(ptr);
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).load(
                            memory,
                            2,
                            true,
                            memarg.offset as u32,
                            memarg.align as u32,
                            ptr,
                            Type::I64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64Load16U { memarg } => {
                    let ptr = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(ptr);
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).load(
                            memory,
                            2,
                            false,
                            memarg.offset as u32,
                            memarg.align as u32,
                            ptr,
                            Type::I64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64Load32S { memarg } => {
                    let ptr = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(ptr);
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).load(
                            memory,
                            4,
                            true,
                            memarg.offset as u32,
                            memarg.align as u32,
                            ptr,
                            Type::I64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64Load32U { memarg } => {
                    let ptr = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(ptr);
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).load(
                            memory,
                            4,
                            false,
                            memarg.offset as u32,
                            memarg.align as u32,
                            ptr,
                            Type::I64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32Store { memarg } => {
                    let value = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(value);
                    let ptr = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(ptr);
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(self.record(
                        IrBuilder::new(&mut module.expressions).store(
                            memory,
                            4,
                            memarg.offset as u32,
                            memarg.align as u32,
                            ptr,
                            value,
                        ),
                        offset,
                    ));
                }
                Operator::I64Store { memarg } => {
                    let value = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(value);
                    let ptr = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(ptr);
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(self.record(
                        IrBuilder::new(&mut module.expressions).store(
                            memory,
                            8,
                            memarg.offset as u32,
                            memarg.align as u32,
                            ptr,
                            value,
                        ),
                        offset,
                    ));
                }
                Operator::F32Store { memarg } => {
                    let value = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(value);
                    let ptr = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(ptr);
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(self.record(
                        IrBuilder::new(&mut module.expressions).store(
                            memory,
                            4,
                            memarg.offset as u32,
                            memarg.align as u32,
                            ptr,
                            value,
                        ),
                        offset,
                    ));
                }
                Operator::F64Store { memarg } => {
                    let value = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(value);
                    let ptr = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(ptr);
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(self.record(
                        IrBuilder::new(&mut module.expressions).store(
                            memory,
                            8,
                            memarg.offset as u32,
                            memarg.align as u32,
                            ptr,
                            value,
                        ),
                        offset,
                    ));
                }
                Operator::I32Store8 { memarg } => {
                    let value = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(value);
                    let ptr = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(ptr);
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(self.record(
                        IrBuilder::new(&mut module.expressions).store(
                            memory,
                            1,
                            memarg.offset as u32,
                            memarg.align as u32,
                            ptr,
                            value,
                        ),
                        offset,
                    ));
                }
                Operator::I32Store16 { memarg } => {
                    let value = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(value);
                    let ptr = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(ptr);
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(self.record(
                        IrBuilder::new(&mut module.expressions).store(
                            memory,
                            2,
                            memarg.offset as u32,
                            memarg.align as u32,
                            ptr,
                            value,
                        ),
                        offset,
                    ));
                }
                Operator::I64Store8 { memarg } => {
                    let value = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(value);
                    let ptr = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(ptr);
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(self.record(
                        IrBuilder::new(&mut module.expressions).store(
                            memory,
                            1,
                            memarg.offset as u32,
                            memarg.align as u32,
                            ptr,
                            value,
                        ),
                        offset,
                    ));
                }
                Operator::I64Store16 { memarg } => {
                    let value = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(value);
                    let ptr = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(ptr);
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(self.record(
                        IrBuilder::new(&mut module.expressions).store(
                            memory,
                            2,
                            memarg.offset as u32,
                            memarg.align as u32,
                            ptr,
                            value,
                        ),
                        offset,
                    ));
                }
                Operator::I64Store32 { memarg } => {
                    let value = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(value);
                    let ptr = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(ptr);
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(self.record(
                        IrBuilder::new(&mut module.expressions).store(
                            memory,
                            4,
                            memarg.offset as u32,
                            memarg.align as u32,
                            ptr,
                            value,
                        ),
                        offset,
                    ));
                }
                Operator::MemorySize { mem, .. } => {
                    let memory = self
                        .memory_ids
                        .get(mem as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", mem))?;
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).memory_size(memory),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::MemoryGrow { mem, .. } => {
                    let delta = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(delta);
                    let memory = self
                        .memory_ids
                        .get(mem as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", mem))?;
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).memory_grow(memory, delta),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32Const { value } => {
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).const_(Literal::I32(value)),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64Const { value } => {
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).const_(Literal::I64(value)),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F32Const { value } => {
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions)
                            .const_(Literal::F32(f32::from_bits(value.bits()))),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F64Const { value } => {
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions)
                            .const_(Literal::F64(f64::from_bits(value.bits()))),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32Add => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::AddInt32,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32Sub => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::SubInt32,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32Mul => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::MulInt32,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32DivS => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::DivSInt32,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32DivU => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::DivUInt32,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32RemS => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::RemSInt32,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32RemU => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::RemUInt32,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32And => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::AndInt32,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32Or => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::OrInt32,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32Xor => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::XorInt32,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32Shl => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::ShlInt32,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32ShrS => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::ShrSInt32,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32ShrU => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::ShrUInt32,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32Rotl => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::RotLInt32,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32Rotr => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::RotRInt32,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64Add => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::AddInt64,
                            left,
                            right,
                            Type::I64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64Sub => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::SubInt64,
                            left,
                            right,
                            Type::I64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64Mul => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::MulInt64,
                            left,
                            right,
                            Type::I64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64DivS => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::DivSInt64,
                            left,
                            right,
                            Type::I64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64DivU => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::DivUInt64,
                            left,
                            right,
                            Type::I64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64RemS => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::RemSInt64,
                            left,
                            right,
                            Type::I64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64RemU => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::RemUInt64,
                            left,
                            right,
                            Type::I64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64And => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::AndInt64,
                            left,
                            right,
                            Type::I64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64Or => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::OrInt64,
                            left,
                            right,
                            Type::I64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64Xor => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::XorInt64,
                            left,
                            right,
                            Type::I64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64Shl => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::ShlInt64,
                            left,
                            right,
                            Type::I64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64ShrS => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::ShrSInt64,
                            left,
                            right,
                            Type::I64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64ShrU => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::ShrUInt64,
                            left,
                            right,
                            Type::I64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64Rotl => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::RotLInt64,
                            left,
                            right,
                            Type::I64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64Rotr => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::RotRInt64,
                            left,
                            right,
                            Type::I64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32Eq => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::EqInt32,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32Ne => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::NeInt32,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32LtS => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::LtSInt32,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32LtU => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::LtUInt32,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32GtS => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::GtSInt32,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32GtU => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::GtUInt32,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32LeS => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::LeSInt32,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32LeU => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::LeUInt32,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32GeS => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::GeSInt32,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32GeU => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::GeUInt32,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64Eq => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::EqInt64,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64Ne => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::NeInt64,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64LtS => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::LtSInt64,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64LtU => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::LtUInt64,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64GtS => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::GtSInt64,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64GtU => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::GtUInt64,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64LeS => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::LeSInt64,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64LeU => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::LeUInt64,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64GeS => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::GeSInt64,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64GeU => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::GeUInt64,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F32Add => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::AddFloat32,
                            left,
                            right,
                            Type::F32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F32Sub => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::SubFloat32,
                            left,
                            right,
                            Type::F32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F32Mul => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::MulFloat32,
                            left,
                            right,
                            Type::F32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F32Div => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::DivFloat32,
                            left,
                            right,
                            Type::F32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F32Min => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::MinFloat32,
                            left,
                            right,
                            Type::F32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F32Max => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::MaxFloat32,
                            left,
                            right,
                            Type::F32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F32Copysign => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::CopySignFloat32,
                            left,
                            right,
                            Type::F32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F64Add => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::AddFloat64,
                            left,
                            right,
                            Type::F64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F64Sub => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::SubFloat64,
                            left,
                            right,
                            Type::F64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F64Mul => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::MulFloat64,
                            left,
                            right,
                            Type::F64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F64Div => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::DivFloat64,
                            left,
                            right,
                            Type::F64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F64Min => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::MinFloat64,
                            left,
                            right,
                            Type::F64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F64Max => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::MaxFloat64,
                            left,
                            right,
                            Type::F64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F64Copysign => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::CopySignFloat64,
                            left,
                            right,
                            Type::F64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F32Eq => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::EqFloat32,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F32Ne => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::NeFloat32,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F32Lt => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::LtFloat32,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F32Gt => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::GtFloat32,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F32Le => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::LeFloat32,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F32Ge => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::GeFloat32,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F64Eq => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::EqFloat64,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F64Ne => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::NeFloat64,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F64Lt => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::LtFloat64,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F64Gt => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::GtFloat64,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F64Le => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::LeFloat64,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F64Ge => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).binary(
                            crate::ir::ops::BinaryOp::GeFloat64,
                            left,
                            right,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32Clz => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::ClzInt32,
                            val,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32Ctz => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::CtzInt32,
                            val,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32Popcnt => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::PopcntInt32,
                            val,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32Eqz => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::EqZInt32,
                            val,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64Clz => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::ClzInt64,
                            val,
                            Type::I64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64Ctz => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::CtzInt64,
                            val,
                            Type::I64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64Popcnt => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::PopcntInt64,
                            val,
                            Type::I64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64Eqz => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::EqZInt64,
                            val,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F32Abs => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::AbsFloat32,
                            val,
                            Type::F32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F32Neg => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::NegFloat32,
                            val,
                            Type::F32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F32Ceil => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::CeilFloat32,
                            val,
                            Type::F32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F32Floor => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::FloorFloat32,
                            val,
                            Type::F32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F32Trunc => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::TruncFloat32,
                            val,
                            Type::F32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F32Nearest => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::NearestFloat32,
                            val,
                            Type::F32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F32Sqrt => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::SqrtFloat32,
                            val,
                            Type::F32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F64Abs => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::AbsFloat64,
                            val,
                            Type::F64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F64Neg => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::NegFloat64,
                            val,
                            Type::F64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F64Ceil => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::CeilFloat64,
                            val,
                            Type::F64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F64Floor => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::FloorFloat64,
                            val,
                            Type::F64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F64Trunc => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::TruncFloat64,
                            val,
                            Type::F64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F64Nearest => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::NearestFloat64,
                            val,
                            Type::F64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F64Sqrt => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::SqrtFloat64,
                            val,
                            Type::F64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32WrapI64 => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::WrapInt64,
                            val,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64ExtendI32S => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::ExtendSInt32,
                            val,
                            Type::I64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64ExtendI32U => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::ExtendUInt32,
                            val,
                            Type::I64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32TruncF32S => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::TruncSFloat32ToInt32,
                            val,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32TruncF32U => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::TruncUFloat32ToInt32,
                            val,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32TruncF64S => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::TruncSFloat64ToInt32,
                            val,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32TruncF64U => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::TruncUFloat64ToInt32,
                            val,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64TruncF32S => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::TruncSFloat32ToInt64,
                            val,
                            Type::I64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64TruncF32U => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::TruncUFloat32ToInt64,
                            val,
                            Type::I64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64TruncF64S => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::TruncSFloat64ToInt64,
                            val,
                            Type::I64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64TruncF64U => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::TruncUFloat64ToInt64,
                            val,
                            Type::I64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F32ConvertI32S => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::ConvertSInt32ToFloat32,
                            val,
                            Type::F32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F32ConvertI32U => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::ConvertUInt32ToFloat32,
                            val,
                            Type::F32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F32ConvertI64S => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::ConvertSInt64ToFloat32,
                            val,
                            Type::F32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F32ConvertI64U => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::ConvertUInt64ToFloat32,
                            val,
                            Type::F32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F64ConvertI32S => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::ConvertSInt32ToFloat64,
                            val,
                            Type::F64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F64ConvertI32U => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::ConvertUInt32ToFloat64,
                            val,
                            Type::F64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F64ConvertI64S => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::ConvertSInt64ToFloat64,
                            val,
                            Type::F64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F64ConvertI64U => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::ConvertUInt64ToFloat64,
                            val,
                            Type::F64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F32DemoteF64 => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::DemoteFloat64,
                            val,
                            Type::F32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F64PromoteF32 => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::PromoteFloat32,
                            val,
                            Type::F64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32ReinterpretF32 => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::ReinterpretFloat32,
                            val,
                            Type::I32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64ReinterpretF64 => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::ReinterpretFloat64,
                            val,
                            Type::I64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F32ReinterpretI32 => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::ReinterpretInt32,
                            val,
                            Type::F32,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F64ReinterpretI64 => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).unary(
                            crate::ir::ops::UnaryOp::ReinterpretInt64,
                            val,
                            Type::F64,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::DataDrop { data_index } => {
                    let id = self
                        .data_ids
                        .get(data_index as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("data_drop", data_index))?;
                    stack.push(self.record(
                        IrBuilder::new(&mut module.expressions).data_drop(id),
                        offset,
                    ));
                }
                Operator::MemoryInit { data_index, mem } => {
                    let size = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(size);
                    let src = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(src);
                    let dest = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(dest);
                    let memory = self
                        .memory_ids
                        .get(mem as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", mem))?;
                    let data_id = self
                        .data_ids
                        .get(data_index as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory_init data", data_index))?;
                    stack.push(
                        self.record(
                            IrBuilder::new(&mut module.expressions)
                                .memory_init(memory, data_id, dest, src, size),
                            offset,
                        ),
                    );
                }
                Operator::MemoryCopy { dst_mem, src_mem } => {
                    let size = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(size);
                    let src = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(src);
                    let dest = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(dest);
                    let dst_id = self
                        .memory_ids
                        .get(dst_mem as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", dst_mem))?;
                    let src_id = self
                        .memory_ids
                        .get(src_mem as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", src_mem))?;
                    stack.push(
                        self.record(
                            IrBuilder::new(&mut module.expressions)
                                .memory_copy(dst_id, src_id, dest, src, size),
                            offset,
                        ),
                    );
                }
                Operator::MemoryFill { mem } => {
                    let size = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(size);
                    let value = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(value);
                    let dest = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(dest);
                    let memory = self
                        .memory_ids
                        .get(mem as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", mem))?;
                    stack.push(
                        self.record(
                            IrBuilder::new(&mut module.expressions)
                                .memory_fill(memory, dest, value, size),
                            offset,
                        ),
                    );
                }
                Operator::RefNull { hty } => {
                    let ty = match hty {
                        wasmparser::HeapType::Abstract { shared: _, ty } => match ty {
                            wasmparser::AbstractHeapType::Func => Type::FUNCREF,
                            wasmparser::AbstractHeapType::Extern => Type::EXTERNREF,
                            wasmparser::AbstractHeapType::Any => Type::ANYREF,
                            wasmparser::AbstractHeapType::Eq => Type::EQREF,
                            wasmparser::AbstractHeapType::I31 => Type::I31REF,
                            wasmparser::AbstractHeapType::Struct => Type::STRUCTREF,
                            wasmparser::AbstractHeapType::Array => Type::ARRAYREF,
                            wasmparser::AbstractHeapType::None => Type::NONE,
                            _ => Type::ANYREF,
                        },
                        _ => Type::ANYREF,
                    };
                    let expr =
                        self.record(IrBuilder::new(&mut module.expressions).ref_null(ty), offset);
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::RefIsNull => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).ref_is_null(val),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::RefFunc { function_index } => {
                    let func_id = self
                        .func_ids
                        .get(function_index as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("function", function_index))?;
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).ref_func(func_id),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::RefEq => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).ref_eq(left, right),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::TableGet { table } => {
                    let index = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(index);
                    let table_id = self
                        .table_ids
                        .get(table as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("table", table))?;
                    let ty = module.tables[table_id].limits.element_type;
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).table_get(table_id, index, ty),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::TableSet { table } => {
                    let value = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(value);
                    let index = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(index);
                    let table_id = self
                        .table_ids
                        .get(table as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("table", table))?;
                    stack.push(self.record(
                        IrBuilder::new(&mut module.expressions).table_set(table_id, index, value),
                        offset,
                    ));
                }
                Operator::TableSize { table } => {
                    let table_id = self
                        .table_ids
                        .get(table as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("table", table))?;
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).table_size(table_id),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::TableGrow { table } => {
                    let delta = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(delta);
                    let value = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(value);
                    let table_id = self
                        .table_ids
                        .get(table as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("table", table))?;
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).table_grow(table_id, value, delta),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::TableFill { table } => {
                    let size = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(size);
                    let value = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(value);
                    let dest = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(dest);
                    let table_id = self
                        .table_ids
                        .get(table as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("table", table))?;
                    stack.push(
                        self.record(
                            IrBuilder::new(&mut module.expressions)
                                .table_fill(table_id, dest, value, size),
                            offset,
                        ),
                    );
                }
                Operator::TableCopy {
                    dst_table,
                    src_table,
                } => {
                    let size = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(size);
                    let src = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(src);
                    let dest = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(dest);
                    let dst_id = self
                        .table_ids
                        .get(dst_table as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("table", dst_table))?;
                    let src_id = self
                        .table_ids
                        .get(src_table as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("table", src_table))?;
                    stack.push(
                        self.record(
                            IrBuilder::new(&mut module.expressions)
                                .table_copy(dst_id, src_id, dest, src, size),
                            offset,
                        ),
                    );
                }
                Operator::TableInit { elem_index, table } => {
                    let size = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(size);
                    let offset_val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(offset_val);
                    let dest = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(dest);
                    let table_id = self
                        .table_ids
                        .get(table as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("table", table))?;
                    let elem_id = self
                        .element_ids
                        .get(elem_index as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("table_init elem", elem_index))?;
                    stack.push(
                        self.record(
                            IrBuilder::new(&mut module.expressions)
                                .table_init(table_id, elem_id, dest, offset_val, size),
                            offset,
                        ),
                    );
                }
                Operator::ElemDrop { elem_index } => {
                    let id = self
                        .element_ids
                        .get(elem_index as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("elem_drop", elem_index))?;
                    stack.push(self.record(
                        IrBuilder::new(&mut module.expressions).elem_drop(id),
                        offset,
                    ));
                }
                Operator::RefI31 => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr =
                        self.record(IrBuilder::new(&mut module.expressions).i31_new(val), offset);
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I31GetS => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).i31_get(val, true),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I31GetU => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).i31_get(val, false),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::StructNew { struct_type_index } => {
                    let ty = self
                        .types
                        .get(struct_type_index as usize)
                        .cloned()
                        .unwrap_or((vec![], vec![]));
                    let param_count = ty.0.len();
                    let mut operands = Vec::with_capacity(param_count);
                    for _ in 0..param_count {
                        let op = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                        consumed.insert(op);
                        operands.push(op);
                    }
                    operands.reverse();
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions)
                            .struct_new(Type::new(HeapType::STRUCT, true), operands),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                    // TODO: refine type
                }
                Operator::StructGet {
                    struct_type_index: _,
                    field_index,
                } => {
                    let ptr = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(ptr);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).struct_get(
                            Type::new(HeapType::STRUCT, true),
                            ptr,
                            field_index,
                            true,
                            Type::ANYREF,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::StructSet {
                    struct_type_index: _,
                    field_index,
                } => {
                    let value = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(value);
                    let ptr = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(ptr);
                    stack.push(self.record(
                        IrBuilder::new(&mut module.expressions).struct_set(
                            Type::new(HeapType::STRUCT, true),
                            ptr,
                            field_index,
                            value,
                        ),
                        offset,
                    ));
                }
                Operator::ArrayNew {
                    array_type_index: _,
                } => {
                    let size = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(size);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).array_new(
                            Type::new(HeapType::ARRAY, true),
                            size,
                            None,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::ArrayGet {
                    array_type_index: _,
                } => {
                    let index = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(index);
                    let ptr = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(ptr);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).array_get(
                            Type::new(HeapType::ARRAY, true),
                            ptr,
                            index,
                            true,
                            Type::ANYREF,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::ArraySet {
                    array_type_index: _,
                } => {
                    let value = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(value);
                    let index = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(index);
                    let ptr = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(ptr);
                    stack.push(self.record(
                        IrBuilder::new(&mut module.expressions).array_set(
                            Type::new(HeapType::ARRAY, true),
                            ptr,
                            index,
                            value,
                        ),
                        offset,
                    ));
                }
                Operator::ArrayLen => {
                    let ptr = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(ptr);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).array_len(ptr),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::RefTestNonNull { hty: _ } | Operator::RefTestNullable { hty: _ } => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).ref_test(
                            crate::ir::ops::RefTestOp::Ref,
                            val,
                            Type::ANYREF,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                    // TODO
                }
                Operator::RefCastNonNull { hty: _ } | Operator::RefCastNullable { hty: _ } => {
                    let val = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(val);
                    let expr = self.record(
                        IrBuilder::new(&mut module.expressions).ref_cast(
                            crate::ir::ops::RefCastOp::Cast,
                            val,
                            Type::ANYREF,
                        ),
                        offset,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                _ => {
                    // Skip unknown opcodes for now
                }
            }
        }

        if !frames.is_empty() {
            return Err(ParseError::UnexpectedEof);
        }

        let mut final_stack: Vec<ExpressionId> = stack
            .into_inner()
            .into_iter()
            .filter(|e| !consumed.contains(e))
            .collect();

        /*
                println!("FINAL STACK: {} items", final_stack.len());
                for (i, expr_id) in final_stack.iter().enumerate() {
                    println!(
                        "  [{}] {:?}: {:?}",
                        i, expr_id, module.expressions[*expr_id].kind
                    );
                }
        */

        if final_stack.is_empty() {
            Ok(Some(
                self.record(IrBuilder::new(&mut module.expressions).nop(), 0),
            ))
        } else if final_stack.len() > 1 {
            let result_type = final_stack
                .last()
                .map(|&e| module.expressions[e].type_)
                .unwrap_or(Type::NONE);
            Ok(Some(self.record(
                IrBuilder::new(&mut module.expressions).block("", final_stack, result_type),
                0,
            )))
        } else {
            let res = final_stack.pop();
            Ok(res)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::expression::ExpressionKind;

    #[test]
    fn test_parse_minimal_module() {
        let wasm = vec![
            0x00, 0x61, 0x73, 0x6D, // magic
            0x01, 0x00, 0x00, 0x00, // version
            // Type section
            0x01, 0x05, // section 1, size 5
            0x01, // 1 type
            0x60, 0x00, 0x01, 0x7F, // func type: () -> i32
            // Function section
            0x03, 0x02, // section 3, size 2
            0x01, 0x00, // 1 function, type 0
            // Code section
            0x0A, 0x06, // section 10, size 6
            0x01, // 1 code
            0x04, // body size 4
            0x00, // 0 locals
            0x41, 0x2A, // i32.const 42
            0x0B, // end
        ];

        let mut reader = BinaryReader::new();
        let module = reader.parse_module(&wasm).expect("Failed to parse module");

        assert_eq!(module.functions.len(), 1);
        let func = &module.functions.iter().next().unwrap().1;
        assert_eq!(func.results, Type::I32);

        if let Some(body) = func.body() {
            match &module.expressions[body].kind {
                ExpressionKind::Const(Literal::I32(42)) => {} // Success
                _ => panic!(
                    "Expected i32.const 42, got {:?}",
                    module.expressions[body].kind
                ),
            }
        } else {
            panic!("Function body is None");
        }
    }

    #[test]
    fn test_parse_multi_param_function() {
        let wasm = vec![
            0x00, 0x61, 0x73, 0x6D, // magic
            0x01, 0x00, 0x00, 0x00, // version
            // Type section
            0x01, 0x07, // section 1, size 7
            0x01, // 1 type
            0x60, 0x02, 0x7F, 0x7F, 0x01, 0x7F, // func type: (i32, i32) -> i32
            // Function section
            0x03, 0x02, // section 3, size 2
            0x01, 0x00, // 1 function, type 0
            // Code section
            0x0A, 0x06, // section 10, size 6
            0x01, // 1 code
            0x04, // body size 4
            0x00, // 0 locals
            0x20, 0x00, // local.get 0
            0x0B, // end
        ];

        let mut reader = BinaryReader::new();
        let module = reader.parse_module(&wasm).expect("Failed to parse module");

        assert_eq!(module.functions.len(), 1);
        let func = &module.functions.iter().next().unwrap().1;

        let expected_params = crate::ir::type_store::intern_tuple(vec![Type::I32, Type::I32]);
        assert_eq!(func.params, expected_params);
        assert_eq!(func.results, Type::I32);
    }
}
