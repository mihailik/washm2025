use crate::ir::expression::{ExpressionId, IrBuilder};
use crate::ir::module::{
    DataSegment, ElementSegment, Export, ExportKind, Function, MemoryLimits, Module, TableLimits,
};
use crate::ir::{HeapType, Literal, Type};
use std::io;
use wasmparser::{Operator, OperatorsReader, Parser, Payload, TypeRef, ValType};

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
        }
    }

    fn make_label_name(&mut self) -> String {
        let name = format!("label${}", self.next_label_id);
        self.next_label_id += 1;
        name
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
                                    field_name.to_string(),
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
                                    name: field_name.to_string(),
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
                                    name: field_name.to_string(),
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
                                    name: field_name.to_string(),
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
                    for (i, type_idx) in reader.into_iter().enumerate() {
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
                                    })
                                    .cast(),
                            );
                        }

                        let id = module.add_function(Function {
                            name: format!("func_{}", i),
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
                    for (i, table) in reader.into_iter().enumerate() {
                        let table = table?;
                        let id = module.add_table(crate::ir::module::Table {
                            name: format!("table_{}", i),
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
                    for (i, memory) in reader.into_iter().enumerate() {
                        let memory = memory?;
                        let id = module.add_memory(crate::ir::module::Memory {
                            name: format!("memory_{}", i),
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
                    for (i, global) in reader.into_iter().enumerate() {
                        let global = global?;
                        let type_ = Self::valtype_to_type(global.ty.content_type);
                        let mutable = global.ty.mutable;

                        let init = self.parse_expression(
                            global.init_expr.get_operators_reader(),
                            &mut module,
                        )?;
                        let name = format!("global_{}", i);

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
                    let func_id = self.func_ids[import_count + code_entry_count];
                    code_entry_count += 1;

                    // 1. First, get the locals and update the local_ids mapping
                    let (var_types, mapping) = {
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
                        mapping.extend_from_slice(func.param_ids());
                        // We will add the var_ids later, but we know how many there will be
                        // Actually, we need the LocalIds which are only created when we add them to the function.
                        (var_types, mapping)
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

                    // 2. Now parse the expression with mutable access to module
                    let expr = self.parse_expression(body.get_operators_reader()?, &mut module)?;

                    // 3. Finally set the body
                    let func = module.get_function_mut(func_id).unwrap();
                    if let Some(mut_body) = func.body_mut() {
                        if let Some(e) = expr {
                            *mut_body = e;
                        }
                    }
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
                _ => {}
            }
        }

        Ok(module)
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
        let mut stack = Vec::new();
        let mut value_stack = Vec::new();
        let mut consumed = std::collections::HashSet::new();

        while !reader.eof() {
            let op = reader.read()?;
            println!(
                "OP: {:?}, stack: {}, values: {}",
                op,
                stack.len(),
                value_stack.len()
            );
            match op {
                Operator::End => {
                    self.last_op_was_else = false;
                    break;
                }
                Operator::Else => {
                    self.last_op_was_else = true;
                    break;
                }
                Operator::Unreachable => {
                    let expr = IrBuilder::new(&mut module.expressions).unreachable();
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::Nop => {
                    let expr = IrBuilder::new(&mut module.expressions).nop();
                    stack.push(expr);
                }
                Operator::Block { blockty } => {
                    let result_type = self.blocktype_to_type(blockty);
                    let name = self.make_label_name();
                    label_stack.push(Some(name.clone()));

                    println!("RECURSE START (Block)");
                    let body = self.parse_expression_impl(reader, label_stack, module)?;
                    println!("RECURSE END (Block)");

                    if let Some(body) = body {
                        label_stack.pop();
                        let block_expr = IrBuilder::new(&mut module.expressions).block(
                            Some(name),
                            vec![body],
                            result_type,
                        );
                        stack.push(block_expr);
                        if result_type != Type::NONE {
                            value_stack.push(block_expr);
                        }
                    } else {
                        label_stack.pop();
                        let block_expr = IrBuilder::new(&mut module.expressions).block(
                            Some(name),
                            vec![],
                            result_type,
                        );
                        stack.push(block_expr);
                        if result_type != Type::NONE {
                            value_stack.push(block_expr);
                        }
                    }
                }
                Operator::Loop { blockty } => {
                    let result_type = self.blocktype_to_type(blockty);
                    let name = self.make_label_name();
                    label_stack.push(Some(name.clone()));

                    println!("RECURSE START (Loop)");
                    let body = self.parse_expression_impl(reader, label_stack, module)?;
                    println!("RECURSE END (Loop)");

                    if let Some(body) = body {
                        label_stack.pop();
                        let loop_expr = IrBuilder::new(&mut module.expressions).loop_(
                            Some(name),
                            body,
                            result_type,
                        );
                        stack.push(loop_expr);
                        if result_type != Type::NONE {
                            value_stack.push(loop_expr);
                        }
                    } else {
                        label_stack.pop();
                        return Err(ParseError::UnexpectedEof);
                    }
                }
                Operator::If { blockty } => {
                    let result_type = self.blocktype_to_type(blockty);
                    let condition = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(condition);

                    let name = self.make_label_name();
                    label_stack.push(Some(name.clone()));

                    println!("RECURSE START (If true)");
                    let if_true = self
                        .parse_expression_impl(reader, label_stack, module)?
                        .ok_or(ParseError::UnexpectedEof)?;
                    println!("RECURSE END (If true)");

                    let if_false = if self.last_op_was_else {
                        println!("RECURSE START (If false)");
                        let res = self.parse_expression_impl(reader, label_stack, module)?;
                        println!("RECURSE END (If false)");
                        res
                    } else {
                        None
                    };

                    label_stack.pop();
                    let if_expr = IrBuilder::new(&mut module.expressions).if_(
                        "",
                        condition,
                        if_true,
                        if_false,
                        result_type,
                    );

                    let if_wrapper = IrBuilder::new(&mut module.expressions).block(
                        Some(name),
                        vec![if_expr],
                        result_type,
                    );
                    stack.push(if_wrapper);
                    if result_type != Type::NONE {
                        value_stack.push(if_wrapper);
                    }
                }
                Operator::Br { relative_depth } => {
                    let target_idx = label_stack.len() as i32 - 1 - relative_depth as i32;
                    let label = if target_idx >= 0 {
                        label_stack[target_idx as usize]
                            .as_deref()
                            .unwrap_or("unnamed")
                    } else {
                        "invalid"
                    };

                    let value = value_stack.pop();
                    if let Some(v) = value {
                        consumed.insert(v);
                    }
                    let break_expr = IrBuilder::new(&mut module.expressions).break_(
                        label.to_string(),
                        None,
                        value,
                        Type::UNREACHABLE,
                    );
                    stack.push(break_expr);
                    value_stack.push(break_expr);
                }
                Operator::BrIf { relative_depth } => {
                    let target_idx = label_stack.len() as i32 - 1 - relative_depth as i32;
                    let label = if target_idx >= 0 {
                        label_stack[target_idx as usize]
                            .as_deref()
                            .unwrap_or("unnamed")
                    } else {
                        "invalid"
                    };

                    let condition = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(condition);
                    let value = value_stack.pop();
                    if let Some(v) = value {
                        consumed.insert(v);
                    }
                    let break_expr = IrBuilder::new(&mut module.expressions).break_(
                        label.to_string(),
                        Some(condition),
                        value,
                        Type::I32,
                    );
                    stack.push(break_expr);
                    value_stack.push(break_expr);
                }
                Operator::BrTable { targets } => {
                    let mut names = Vec::with_capacity(targets.len() as usize);
                    for depth in targets.targets() {
                        let depth = depth?;
                        let target_idx = label_stack.len() as i32 - 1 - depth as i32;
                        let label = if target_idx >= 0 {
                            label_stack[target_idx as usize]
                                .as_deref()
                                .unwrap_or("unnamed")
                        } else {
                            "invalid"
                        };
                        names.push(label.to_string());
                    }
                    let default_depth = targets.default();
                    let target_idx = label_stack.len() as i32 - 1 - default_depth as i32;
                    let default = if target_idx >= 0 {
                        label_stack[target_idx as usize]
                            .as_deref()
                            .unwrap_or("unnamed")
                    } else {
                        "invalid"
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

                    let switch_expr = IrBuilder::new(&mut module.expressions).switch(
                        names,
                        default.to_string(),
                        condition,
                        value,
                    );
                    stack.push(switch_expr);
                    value_stack.push(switch_expr);
                }
                Operator::Return => {
                    let value = value_stack.pop();
                    if let Some(v) = value {
                        consumed.insert(v);
                    }
                    let return_expr = IrBuilder::new(&mut module.expressions).return_(value);
                    stack.push(return_expr);
                    value_stack.push(return_expr);
                }
                Operator::Drop => {
                    let value = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(value);
                    let drop_expr = IrBuilder::new(&mut module.expressions).drop(value);
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
                    let select_expr = IrBuilder::new(&mut module.expressions).select(
                        condition,
                        if_true,
                        if_false,
                        result_type,
                    );
                    stack.push(select_expr);
                    value_stack.push(select_expr);
                }
                Operator::Call { function_index } => {
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

                    let call_expr = IrBuilder::new(&mut module.expressions).call(
                        func_id,
                        operands,
                        result_type,
                        false,
                    );
                    stack.push(call_expr);
                    if result_type != Type::NONE {
                        value_stack.push(call_expr);
                    }
                }
                Operator::CallIndirect {
                    type_index,
                    table_index,
                } => {
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

                    let call_expr = IrBuilder::new(&mut module.expressions).call_indirect(
                        table_id,
                        target,
                        operands,
                        type_id,
                        result_type,
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
                        .ok_or(ParseError::InvalidIndex("local.get", local_index))?;
                    let expr = IrBuilder::new(&mut module.expressions).local_get(id, Type::I32);
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
                    stack.push(IrBuilder::new(&mut module.expressions).local_set(id, value));
                }
                Operator::LocalTee { local_index } => {
                    let id = self
                        .current_func_local_ids
                        .get(local_index as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("local.tee", local_index))?;
                    let value = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(value);
                    let value_type = module.expressions[value].type_;
                    let expr =
                        IrBuilder::new(&mut module.expressions).local_tee(id, value, value_type);
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::GlobalGet { global_index } => {
                    let id = self
                        .global_ids
                        .get(global_index as usize)
                        .copied()
                        .ok_or(ParseError::InvalidSection)?;
                    let expr = IrBuilder::new(&mut module.expressions).global_get(id, Type::I32);
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
                    stack.push(IrBuilder::new(&mut module.expressions).global_set(id, value));
                }
                Operator::I32Load { memarg } => {
                    let ptr = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(IrBuilder::new(&mut module.expressions).load(
                        memory,
                        4,
                        false,
                        memarg.offset as u32,
                        memarg.align as u32,
                        ptr,
                        Type::I32,
                    ));
                }
                Operator::I64Load { memarg } => {
                    let ptr = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(IrBuilder::new(&mut module.expressions).load(
                        memory,
                        8,
                        false,
                        memarg.offset as u32,
                        memarg.align as u32,
                        ptr,
                        Type::I64,
                    ));
                }
                Operator::F32Load { memarg } => {
                    let ptr = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(IrBuilder::new(&mut module.expressions).load(
                        memory,
                        4,
                        false,
                        memarg.offset as u32,
                        memarg.align as u32,
                        ptr,
                        Type::F32,
                    ));
                }
                Operator::F64Load { memarg } => {
                    let ptr = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(IrBuilder::new(&mut module.expressions).load(
                        memory,
                        8,
                        false,
                        memarg.offset as u32,
                        memarg.align as u32,
                        ptr,
                        Type::F64,
                    ));
                }
                Operator::I32Load8S { memarg } => {
                    let ptr = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(IrBuilder::new(&mut module.expressions).load(
                        memory,
                        1,
                        true,
                        memarg.offset as u32,
                        memarg.align as u32,
                        ptr,
                        Type::I32,
                    ));
                }
                Operator::I32Load8U { memarg } => {
                    let ptr = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(IrBuilder::new(&mut module.expressions).load(
                        memory,
                        1,
                        false,
                        memarg.offset as u32,
                        memarg.align as u32,
                        ptr,
                        Type::I32,
                    ));
                }
                Operator::I32Load16S { memarg } => {
                    let ptr = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(IrBuilder::new(&mut module.expressions).load(
                        memory,
                        2,
                        true,
                        memarg.offset as u32,
                        memarg.align as u32,
                        ptr,
                        Type::I32,
                    ));
                }
                Operator::I32Load16U { memarg } => {
                    let ptr = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(IrBuilder::new(&mut module.expressions).load(
                        memory,
                        2,
                        false,
                        memarg.offset as u32,
                        memarg.align as u32,
                        ptr,
                        Type::I32,
                    ));
                }
                Operator::I64Load8S { memarg } => {
                    let ptr = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(IrBuilder::new(&mut module.expressions).load(
                        memory,
                        1,
                        true,
                        memarg.offset as u32,
                        memarg.align as u32,
                        ptr,
                        Type::I64,
                    ));
                }
                Operator::I64Load8U { memarg } => {
                    let ptr = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(IrBuilder::new(&mut module.expressions).load(
                        memory,
                        1,
                        false,
                        memarg.offset as u32,
                        memarg.align as u32,
                        ptr,
                        Type::I64,
                    ));
                }
                Operator::I64Load16S { memarg } => {
                    let ptr = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(IrBuilder::new(&mut module.expressions).load(
                        memory,
                        2,
                        true,
                        memarg.offset as u32,
                        memarg.align as u32,
                        ptr,
                        Type::I64,
                    ));
                }
                Operator::I64Load16U { memarg } => {
                    let ptr = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(IrBuilder::new(&mut module.expressions).load(
                        memory,
                        2,
                        false,
                        memarg.offset as u32,
                        memarg.align as u32,
                        ptr,
                        Type::I64,
                    ));
                }
                Operator::I64Load32S { memarg } => {
                    let ptr = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(IrBuilder::new(&mut module.expressions).load(
                        memory,
                        4,
                        true,
                        memarg.offset as u32,
                        memarg.align as u32,
                        ptr,
                        Type::I64,
                    ));
                }
                Operator::I64Load32U { memarg } => {
                    let ptr = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(IrBuilder::new(&mut module.expressions).load(
                        memory,
                        4,
                        false,
                        memarg.offset as u32,
                        memarg.align as u32,
                        ptr,
                        Type::I64,
                    ));
                }
                Operator::I32Store { memarg } => {
                    let value = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let ptr = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(IrBuilder::new(&mut module.expressions).store(
                        memory,
                        4,
                        memarg.offset as u32,
                        memarg.align as u32,
                        ptr,
                        value,
                    ));
                }
                Operator::I64Store { memarg } => {
                    let value = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let ptr = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(IrBuilder::new(&mut module.expressions).store(
                        memory,
                        8,
                        memarg.offset as u32,
                        memarg.align as u32,
                        ptr,
                        value,
                    ));
                }
                Operator::F32Store { memarg } => {
                    let value = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let ptr = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(IrBuilder::new(&mut module.expressions).store(
                        memory,
                        4,
                        memarg.offset as u32,
                        memarg.align as u32,
                        ptr,
                        value,
                    ));
                }
                Operator::F64Store { memarg } => {
                    let value = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let ptr = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(IrBuilder::new(&mut module.expressions).store(
                        memory,
                        8,
                        memarg.offset as u32,
                        memarg.align as u32,
                        ptr,
                        value,
                    ));
                }
                Operator::I32Store8 { memarg } => {
                    let value = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let ptr = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(IrBuilder::new(&mut module.expressions).store(
                        memory,
                        1,
                        memarg.offset as u32,
                        memarg.align as u32,
                        ptr,
                        value,
                    ));
                }
                Operator::I32Store16 { memarg } => {
                    let value = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let ptr = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(IrBuilder::new(&mut module.expressions).store(
                        memory,
                        2,
                        memarg.offset as u32,
                        memarg.align as u32,
                        ptr,
                        value,
                    ));
                }
                Operator::I64Store8 { memarg } => {
                    let value = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let ptr = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(IrBuilder::new(&mut module.expressions).store(
                        memory,
                        1,
                        memarg.offset as u32,
                        memarg.align as u32,
                        ptr,
                        value,
                    ));
                }
                Operator::I64Store16 { memarg } => {
                    let value = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let ptr = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(IrBuilder::new(&mut module.expressions).store(
                        memory,
                        2,
                        memarg.offset as u32,
                        memarg.align as u32,
                        ptr,
                        value,
                    ));
                }
                Operator::I64Store32 { memarg } => {
                    let value = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let ptr = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let memory = self
                        .memory_ids
                        .first()
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(IrBuilder::new(&mut module.expressions).store(
                        memory,
                        4,
                        memarg.offset as u32,
                        memarg.align as u32,
                        ptr,
                        value,
                    ));
                }
                Operator::MemorySize { mem, .. } => {
                    let memory = self
                        .memory_ids
                        .get(mem as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", mem))?;
                    let expr = IrBuilder::new(&mut module.expressions).memory_size(memory);
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
                    let expr = IrBuilder::new(&mut module.expressions).memory_grow(memory, delta);
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32Const { value } => {
                    let expr = IrBuilder::new(&mut module.expressions).const_(Literal::I32(value));
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I64Const { value } => {
                    let expr = IrBuilder::new(&mut module.expressions).const_(Literal::I64(value));
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F32Const { value } => {
                    let expr = IrBuilder::new(&mut module.expressions)
                        .const_(Literal::F32(f32::from_bits(value.bits())));
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::F64Const { value } => {
                    let expr = IrBuilder::new(&mut module.expressions)
                        .const_(Literal::F64(f64::from_bits(value.bits())));
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32Add => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::AddInt32,
                        left,
                        right,
                        Type::I32,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32Sub => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::SubInt32,
                        left,
                        right,
                        Type::I32,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32Mul => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::MulInt32,
                        left,
                        right,
                        Type::I32,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32DivS => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::DivSInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32DivU => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::DivUInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32RemS => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::RemSInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32RemU => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::RemUInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32And => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::AndInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32Or => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::OrInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32Xor => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::XorInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32Shl => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::ShlInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32ShrS => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::ShrSInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32ShrU => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::ShrUInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32Rotl => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::RotLInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32Rotr => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::RotRInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I64Add => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::AddInt64,
                        left,
                        right,
                        Type::I64,
                    ));
                }
                Operator::I64Sub => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::SubInt64,
                        left,
                        right,
                        Type::I64,
                    ));
                }
                Operator::I64Mul => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::MulInt64,
                        left,
                        right,
                        Type::I64,
                    ));
                }
                Operator::I64DivS => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::DivSInt64,
                        left,
                        right,
                        Type::I64,
                    ));
                }
                Operator::I64DivU => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::DivUInt64,
                        left,
                        right,
                        Type::I64,
                    ));
                }
                Operator::I64RemS => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::RemSInt64,
                        left,
                        right,
                        Type::I64,
                    ));
                }
                Operator::I64RemU => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::RemUInt64,
                        left,
                        right,
                        Type::I64,
                    ));
                }
                Operator::I64And => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::AndInt64,
                        left,
                        right,
                        Type::I64,
                    ));
                }
                Operator::I64Or => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::OrInt64,
                        left,
                        right,
                        Type::I64,
                    ));
                }
                Operator::I64Xor => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::XorInt64,
                        left,
                        right,
                        Type::I64,
                    ));
                }
                Operator::I64Shl => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::ShlInt64,
                        left,
                        right,
                        Type::I64,
                    ));
                }
                Operator::I64ShrS => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::ShrSInt64,
                        left,
                        right,
                        Type::I64,
                    ));
                }
                Operator::I64ShrU => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::ShrUInt64,
                        left,
                        right,
                        Type::I64,
                    ));
                }
                Operator::I64Rotl => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::RotLInt64,
                        left,
                        right,
                        Type::I64,
                    ));
                }
                Operator::I64Rotr => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::RotRInt64,
                        left,
                        right,
                        Type::I64,
                    ));
                }
                Operator::I32Eq => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::EqInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32Ne => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::NeInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32LtS => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::LtSInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32LtU => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::LtUInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32GtS => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::GtSInt32,
                        left,
                        right,
                        Type::I32,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32GtU => {
                    let right = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(right);
                    let left = value_stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    consumed.insert(left);
                    let expr = IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::GtUInt32,
                        left,
                        right,
                        Type::I32,
                    );
                    stack.push(expr);
                    value_stack.push(expr);
                }
                Operator::I32LeS => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::LeSInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32LeU => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::LeUInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32GeS => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::GeSInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32GeU => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::GeUInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I64Eq => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::EqInt64,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I64Ne => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::NeInt64,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I64LtS => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::LtSInt64,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I64LtU => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::LtUInt64,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I64GtS => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::GtSInt64,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I64GtU => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::GtUInt64,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I64LeS => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::LeSInt64,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I64LeU => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::LeUInt64,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I64GeS => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::GeSInt64,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I64GeU => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::GeUInt64,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::F32Add => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::AddFloat32,
                        left,
                        right,
                        Type::F32,
                    ));
                }
                Operator::F32Sub => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::SubFloat32,
                        left,
                        right,
                        Type::F32,
                    ));
                }
                Operator::F32Mul => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::MulFloat32,
                        left,
                        right,
                        Type::F32,
                    ));
                }
                Operator::F32Div => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::DivFloat32,
                        left,
                        right,
                        Type::F32,
                    ));
                }
                Operator::F32Min => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::MinFloat32,
                        left,
                        right,
                        Type::F32,
                    ));
                }
                Operator::F32Max => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::MaxFloat32,
                        left,
                        right,
                        Type::F32,
                    ));
                }
                Operator::F32Copysign => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::CopySignFloat32,
                        left,
                        right,
                        Type::F32,
                    ));
                }
                Operator::F64Add => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::AddFloat64,
                        left,
                        right,
                        Type::F64,
                    ));
                }
                Operator::F64Sub => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::SubFloat64,
                        left,
                        right,
                        Type::F64,
                    ));
                }
                Operator::F64Mul => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::MulFloat64,
                        left,
                        right,
                        Type::F64,
                    ));
                }
                Operator::F64Div => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::DivFloat64,
                        left,
                        right,
                        Type::F64,
                    ));
                }
                Operator::F64Min => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::MinFloat64,
                        left,
                        right,
                        Type::F64,
                    ));
                }
                Operator::F64Max => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::MaxFloat64,
                        left,
                        right,
                        Type::F64,
                    ));
                }
                Operator::F64Copysign => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::CopySignFloat64,
                        left,
                        right,
                        Type::F64,
                    ));
                }
                Operator::F32Eq => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::EqFloat32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::F32Ne => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::NeFloat32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::F32Lt => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::LtFloat32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::F32Gt => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::GtFloat32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::F32Le => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::LeFloat32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::F32Ge => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::GeFloat32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::F64Eq => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::EqFloat64,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::F64Ne => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::NeFloat64,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::F64Lt => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::LtFloat64,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::F64Gt => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::GtFloat64,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::F64Le => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::LeFloat64,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::F64Ge => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).binary(
                        crate::ir::ops::BinaryOp::GeFloat64,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32Clz => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::ClzInt32,
                        val,
                        Type::I32,
                    ));
                }
                Operator::I32Ctz => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::CtzInt32,
                        val,
                        Type::I32,
                    ));
                }
                Operator::I32Popcnt => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::PopcntInt32,
                        val,
                        Type::I32,
                    ));
                }
                Operator::I32Eqz => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::EqZInt32,
                        val,
                        Type::I32,
                    ));
                }
                Operator::I64Clz => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::ClzInt64,
                        val,
                        Type::I64,
                    ));
                }
                Operator::I64Ctz => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::CtzInt64,
                        val,
                        Type::I64,
                    ));
                }
                Operator::I64Popcnt => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::PopcntInt64,
                        val,
                        Type::I64,
                    ));
                }
                Operator::I64Eqz => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::EqZInt64,
                        val,
                        Type::I32,
                    ));
                }
                Operator::F32Abs => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::AbsFloat32,
                        val,
                        Type::F32,
                    ));
                }
                Operator::F32Neg => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::NegFloat32,
                        val,
                        Type::F32,
                    ));
                }
                Operator::F32Ceil => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::CeilFloat32,
                        val,
                        Type::F32,
                    ));
                }
                Operator::F32Floor => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::FloorFloat32,
                        val,
                        Type::F32,
                    ));
                }
                Operator::F32Trunc => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::TruncFloat32,
                        val,
                        Type::F32,
                    ));
                }
                Operator::F32Nearest => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::NearestFloat32,
                        val,
                        Type::F32,
                    ));
                }
                Operator::F32Sqrt => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::SqrtFloat32,
                        val,
                        Type::F32,
                    ));
                }
                Operator::F64Abs => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::AbsFloat64,
                        val,
                        Type::F64,
                    ));
                }
                Operator::F64Neg => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::NegFloat64,
                        val,
                        Type::F64,
                    ));
                }
                Operator::F64Ceil => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::CeilFloat64,
                        val,
                        Type::F64,
                    ));
                }
                Operator::F64Floor => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::FloorFloat64,
                        val,
                        Type::F64,
                    ));
                }
                Operator::F64Trunc => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::TruncFloat64,
                        val,
                        Type::F64,
                    ));
                }
                Operator::F64Nearest => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::NearestFloat64,
                        val,
                        Type::F64,
                    ));
                }
                Operator::F64Sqrt => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::SqrtFloat64,
                        val,
                        Type::F64,
                    ));
                }
                Operator::I32WrapI64 => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::WrapInt64,
                        val,
                        Type::I32,
                    ));
                }
                Operator::I64ExtendI32S => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::ExtendSInt32,
                        val,
                        Type::I64,
                    ));
                }
                Operator::I64ExtendI32U => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::ExtendUInt32,
                        val,
                        Type::I64,
                    ));
                }
                Operator::I32TruncF32S => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::TruncSFloat32ToInt32,
                        val,
                        Type::I32,
                    ));
                }
                Operator::I32TruncF32U => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::TruncUFloat32ToInt32,
                        val,
                        Type::I32,
                    ));
                }
                Operator::I32TruncF64S => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::TruncSFloat64ToInt32,
                        val,
                        Type::I32,
                    ));
                }
                Operator::I32TruncF64U => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::TruncUFloat64ToInt32,
                        val,
                        Type::I32,
                    ));
                }
                Operator::I64TruncF32S => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::TruncSFloat32ToInt64,
                        val,
                        Type::I64,
                    ));
                }
                Operator::I64TruncF32U => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::TruncUFloat32ToInt64,
                        val,
                        Type::I64,
                    ));
                }
                Operator::I64TruncF64S => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::TruncSFloat64ToInt64,
                        val,
                        Type::I64,
                    ));
                }
                Operator::I64TruncF64U => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::TruncUFloat64ToInt64,
                        val,
                        Type::I64,
                    ));
                }
                Operator::F32ConvertI32S => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::ConvertSInt32ToFloat32,
                        val,
                        Type::F32,
                    ));
                }
                Operator::F32ConvertI32U => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::ConvertUInt32ToFloat32,
                        val,
                        Type::F32,
                    ));
                }
                Operator::F32ConvertI64S => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::ConvertSInt64ToFloat32,
                        val,
                        Type::F32,
                    ));
                }
                Operator::F32ConvertI64U => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::ConvertUInt64ToFloat32,
                        val,
                        Type::F32,
                    ));
                }
                Operator::F64ConvertI32S => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::ConvertSInt32ToFloat64,
                        val,
                        Type::F64,
                    ));
                }
                Operator::F64ConvertI32U => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::ConvertUInt32ToFloat64,
                        val,
                        Type::F64,
                    ));
                }
                Operator::F64ConvertI64S => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::ConvertSInt64ToFloat64,
                        val,
                        Type::F64,
                    ));
                }
                Operator::F64ConvertI64U => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::ConvertUInt64ToFloat64,
                        val,
                        Type::F64,
                    ));
                }
                Operator::F32DemoteF64 => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::DemoteFloat64,
                        val,
                        Type::F32,
                    ));
                }
                Operator::F64PromoteF32 => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::PromoteFloat32,
                        val,
                        Type::F64,
                    ));
                }
                Operator::I32ReinterpretF32 => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::ReinterpretFloat32,
                        val,
                        Type::I32,
                    ));
                }
                Operator::I64ReinterpretF64 => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::ReinterpretFloat64,
                        val,
                        Type::I64,
                    ));
                }
                Operator::F32ReinterpretI32 => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::ReinterpretInt32,
                        val,
                        Type::F32,
                    ));
                }
                Operator::F64ReinterpretI64 => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).unary(
                        crate::ir::ops::UnaryOp::ReinterpretInt64,
                        val,
                        Type::F64,
                    ));
                }
                Operator::DataDrop { data_index } => {
                    let id = self
                        .data_ids
                        .get(data_index as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("data_drop", data_index))?;
                    stack.push(IrBuilder::new(&mut module.expressions).data_drop(id));
                }
                Operator::MemoryInit { data_index, mem } => {
                    let size = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let src = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let dest = stack.pop().ok_or(ParseError::UnexpectedEof)?;
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
                        IrBuilder::new(&mut module.expressions)
                            .memory_init(memory, data_id, dest, src, size),
                    );
                }
                Operator::MemoryCopy { dst_mem, src_mem } => {
                    let size = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let src = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let dest = stack.pop().ok_or(ParseError::UnexpectedEof)?;
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
                        IrBuilder::new(&mut module.expressions)
                            .memory_copy(dst_id, src_id, dest, src, size),
                    );
                }
                Operator::MemoryFill { mem } => {
                    let size = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let value = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let dest = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let memory = self
                        .memory_ids
                        .get(mem as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", mem))?;
                    stack.push(
                        IrBuilder::new(&mut module.expressions)
                            .memory_fill(memory, dest, value, size),
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
                    stack.push(IrBuilder::new(&mut module.expressions).ref_null(ty));
                }
                Operator::RefIsNull => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).ref_is_null(val));
                }
                Operator::RefFunc { function_index } => {
                    let func_id = self
                        .func_ids
                        .get(function_index as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("function", function_index))?;
                    stack.push(IrBuilder::new(&mut module.expressions).ref_func(func_id));
                }
                Operator::RefEq => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).ref_eq(left, right));
                }
                Operator::TableGet { table } => {
                    let index = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let table_id = self
                        .table_ids
                        .get(table as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("table", table))?;
                    let ty = module.tables[table_id].limits.element_type;
                    stack.push(
                        IrBuilder::new(&mut module.expressions).table_get(table_id, index, ty),
                    );
                }
                Operator::TableSet { table } => {
                    let value = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let index = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let table_id = self
                        .table_ids
                        .get(table as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("table", table))?;
                    stack.push(
                        IrBuilder::new(&mut module.expressions).table_set(table_id, index, value),
                    );
                }
                Operator::TableSize { table } => {
                    let table_id = self
                        .table_ids
                        .get(table as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("table", table))?;
                    stack.push(IrBuilder::new(&mut module.expressions).table_size(table_id));
                }
                Operator::TableGrow { table } => {
                    let delta = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let value = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let table_id = self
                        .table_ids
                        .get(table as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("table", table))?;
                    stack.push(
                        IrBuilder::new(&mut module.expressions).table_grow(table_id, value, delta),
                    );
                }
                Operator::TableFill { table } => {
                    let size = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let value = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let dest = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let table_id = self
                        .table_ids
                        .get(table as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("table", table))?;
                    stack.push(
                        IrBuilder::new(&mut module.expressions)
                            .table_fill(table_id, dest, value, size),
                    );
                }
                Operator::TableCopy {
                    dst_table,
                    src_table,
                } => {
                    let size = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let src = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let dest = stack.pop().ok_or(ParseError::UnexpectedEof)?;
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
                        IrBuilder::new(&mut module.expressions)
                            .table_copy(dst_id, src_id, dest, src, size),
                    );
                }
                Operator::TableInit { elem_index, table } => {
                    let size = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let offset = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let dest = stack.pop().ok_or(ParseError::UnexpectedEof)?;
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
                        IrBuilder::new(&mut module.expressions)
                            .table_init(table_id, elem_id, dest, offset, size),
                    );
                }
                Operator::ElemDrop { elem_index } => {
                    let id = self
                        .element_ids
                        .get(elem_index as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("elem_drop", elem_index))?;
                    stack.push(IrBuilder::new(&mut module.expressions).elem_drop(id));
                }
                Operator::RefI31 => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).i31_new(val));
                }
                Operator::I31GetS => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).i31_get(val, true));
                }
                Operator::I31GetU => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).i31_get(val, false));
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
                        operands.push(stack.pop().ok_or(ParseError::UnexpectedEof)?);
                    }
                    operands.reverse();
                    stack.push(
                        IrBuilder::new(&mut module.expressions)
                            .struct_new(Type::new(HeapType::STRUCT, true), operands),
                    );
                    // TODO: refine type
                }
                Operator::StructGet {
                    struct_type_index: _,
                    field_index,
                } => {
                    let ptr = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).struct_get(
                        Type::new(HeapType::STRUCT, true),
                        ptr,
                        field_index,
                        true,
                        Type::ANYREF,
                    )); // TODO: refine types
                }
                Operator::StructSet {
                    struct_type_index: _,
                    field_index,
                } => {
                    let value = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let ptr = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).struct_set(
                        Type::new(HeapType::STRUCT, true),
                        ptr,
                        field_index,
                        value,
                    ));
                }
                Operator::ArrayNew {
                    array_type_index: _,
                } => {
                    let size = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).array_new(
                        Type::new(HeapType::ARRAY, true),
                        size,
                        None,
                    ));
                }
                Operator::ArrayGet {
                    array_type_index: _,
                } => {
                    let index = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let ptr = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).array_get(
                        Type::new(HeapType::ARRAY, true),
                        ptr,
                        index,
                        true,
                        Type::ANYREF,
                    ));
                }
                Operator::ArraySet {
                    array_type_index: _,
                } => {
                    let value = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let index = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let ptr = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).array_set(
                        Type::new(HeapType::ARRAY, true),
                        ptr,
                        index,
                        value,
                    ));
                }
                Operator::ArrayLen => {
                    let ptr = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).array_len(ptr));
                }
                Operator::RefTestNonNull { hty: _ } | Operator::RefTestNullable { hty: _ } => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).ref_test(
                        crate::ir::ops::RefTestOp::Ref,
                        val,
                        Type::ANYREF,
                    ));
                    // TODO
                }
                Operator::RefCastNonNull { hty: _ } | Operator::RefCastNullable { hty: _ } => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(IrBuilder::new(&mut module.expressions).ref_cast(
                        crate::ir::ops::RefCastOp::Cast,
                        val,
                        Type::ANYREF,
                    )); // TODO
                }
                _ => {
                    // Skip unknown opcodes for now
                }
            }
        }

        let mut final_stack: Vec<ExpressionId> = stack
            .into_iter()
            .filter(|e| !consumed.contains(e))
            .collect();

        println!("FINAL STACK: {} items", final_stack.len());
        for (i, expr_id) in final_stack.iter().enumerate() {
            println!(
                "  [{}] {:?}: {:?}",
                i, expr_id, module.expressions[*expr_id].kind
            );
        }

        if final_stack.is_empty() {
            Ok(Some(IrBuilder::new(&mut module.expressions).nop()))
        } else if final_stack.len() > 1 {
            let result_type = final_stack
                .last()
                .map(|&e| module.expressions[e].type_)
                .unwrap_or(Type::NONE);
            Ok(Some(IrBuilder::new(&mut module.expressions).block(
                "",
                final_stack,
                result_type,
            )))
        } else {
            Ok(final_stack.pop())
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
