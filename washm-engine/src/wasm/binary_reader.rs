use crate::ir::expression::{ExprRef, Expression, ExpressionKind, IrBuilder};
use crate::ir::module::{
    DataSegment, ElementSegment, Export, ExportKind, Function, MemoryLimits, Module, TableLimits,
};
use crate::ir::{HeapType, Literal, Type};
use bumpalo::collections::Vec as BumpVec;
use bumpalo::Bump;
use std::io;
use wasmparser::{
    BinaryReader as WasmBinaryReader, Operator, OperatorsReader, Parser, Payload, TypeRef, ValType,
};

pub struct BinaryReader<'a> {
    bump: &'a Bump,
    data: Vec<u8>,
    types: Vec<(Vec<Type>, Vec<Type>)>,
    all_function_types: Vec<u32>,
    next_label_id: u32,
    last_op_was_else: bool,
    import_names: Vec<String>,
    func_ids: Vec<crate::ir::FuncId>,
    table_ids: Vec<crate::ir::TableId>,
    memory_ids: Vec<crate::ir::MemoryId>,
    global_ids: Vec<crate::ir::GlobalId>,
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

impl<'a> BinaryReader<'a> {
    pub fn new(bump: &'a Bump, data: Vec<u8>) -> Self {
        Self {
            bump,
            data,
            types: Vec::new(),
            all_function_types: Vec::new(),
            next_label_id: 0,
            last_op_was_else: false,
            import_names: Vec::new(),
            func_ids: Vec::new(),
            table_ids: Vec::new(),
            memory_ids: Vec::new(),
            global_ids: Vec::new(),
        }
    }

    pub fn read_leb128_i32(&mut self) -> Result<i32> {
        let mut reader = WasmBinaryReader::new(&self.data, 0);
        Ok(reader.read_var_i32()?)
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

    pub fn parse_module(&mut self) -> Result<Module<'a>> {
        let mut module = Module::new(self.bump);
        let mut function_section = Vec::new();
        let mut code_section = Vec::new();

        let this = self as *mut Self;
        for payload in Parser::new(0).parse_all(&self.data) {
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
                                unsafe { (*this).types.push((params, results)) };
                            }
                        }
                    }
                }
                Payload::ImportSection(reader) => {
                    for imports in reader {
                        let imports = imports?;
                        let mut process_import =
                            |module_name: &str, field_name: &str, ty: TypeRef| -> Result<()> {
                                match ty {
                                    TypeRef::Func(type_idx) => {
                                        let func_type = unsafe {
                                            (&(*this).types)
                                                .get(type_idx as usize)
                                                .ok_or(ParseError::InvalidType)?
                                        };
                                        let params = Self::types_to_type(&func_type.0);
                                        let results = Self::types_to_type(&func_type.1);

                                        let id = module.add_function(
                                            crate::ir::module::Function::new_import(
                                                field_name.to_string(),
                                                module_name.to_string(),
                                                field_name.to_string(),
                                                params,
                                                results,
                                            ),
                                        );
                                        self.func_ids.push(id);

                                        unsafe {
                                            (*this).all_function_types.push(type_idx);
                                            (*this).import_names.push(field_name.to_string());
                                        }
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
                                            limits: limits.clone(),
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
                                            limits: limits.clone(),
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
                                };
                                Ok(())
                            };

                        match imports {
                            wasmparser::Imports::Single(_, import) => {
                                process_import(import.module, import.name, import.ty)?;
                            }
                            wasmparser::Imports::Compact1 { module, items } => {
                                for item in items {
                                    let item = item?;
                                    process_import(module, item.name, item.ty)?;
                                }
                            }
                            wasmparser::Imports::Compact2 { module, ty, names } => {
                                for name in names {
                                    let name = name?;
                                    process_import(module, name, ty)?;
                                }
                            }
                        }
                    }
                }
                Payload::FunctionSection(reader) => {
                    for (i, type_idx) in reader.into_iter().enumerate() {
                        let type_idx = type_idx?;
                        function_section.push(type_idx);
                        unsafe { (*this).all_function_types.push(type_idx) };

                        let func_type = unsafe {
                            (&(*this).types)
                                .get(type_idx as usize)
                                .ok_or(ParseError::InvalidType)?
                        };
                        let params = Self::types_to_type(&func_type.0);
                        let results = Self::types_to_type(&func_type.1);

                        let id = module.add_function(Function {
                            name: format!("func_{}", i),
                            params,
                            results,
                            type_idx: Some(type_idx),
                            vars: Vec::new(),
                            body: None,
                            local_names: Vec::new(),
                            import: None,
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

                        let init = unsafe {
                            (*this)
                                .parse_expression(global.init_expr.get_operators_reader(), &module)
                        }?;
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
                            } => unsafe {
                                (*this)
                                    .parse_expression(offset_expr.get_operators_reader(), &module)
                            }?,
                            _ => None,
                        };

                        let mut func_indices = Vec::new();
                        match element.items {
                            wasmparser::ElementItems::Functions(funcs) => {
                                for f in funcs {
                                    let f_idx = f?;
                                    let id =
                                        self.func_ids.get(f_idx as usize).copied().ok_or(
                                            ParseError::InvalidIndex("element func", f_idx),
                                        )?;
                                    func_indices.push(id);
                                }
                            }
                            _ => {}
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

                        module.elements.push(ElementSegment {
                            table_index,
                            offset: offset.ok_or(ParseError::UnexpectedEof)?,
                            func_indices,
                        });
                    }
                }
                Payload::CodeSectionEntry(body) => {
                    let mut locals = Vec::new();
                    let mut reader = body.get_locals_reader()?;
                    for _ in 0..reader.get_count() {
                        let (count, ty) = reader.read()?;
                        let t = Self::valtype_to_type(ty);
                        for _ in 0..count {
                            locals.push(t);
                        }
                    }

                    let expr =
                        unsafe { (*this).parse_expression(body.get_operators_reader()?, &module) }?;
                    code_section.push((locals, expr));
                }
                Payload::DataSection(reader) => {
                    for data in reader {
                        let data = data?;
                        let offset = match data.kind {
                            wasmparser::DataKind::Active {
                                ref offset_expr, ..
                            } => unsafe {
                                (*this)
                                    .parse_expression(offset_expr.get_operators_reader(), &module)
                            }?,
                            _ => None,
                        };

                        let memory_index = match data.kind {
                            wasmparser::DataKind::Active { memory_index, .. } => {
                                self.memory_ids.get(memory_index as usize).copied().ok_or(
                                    ParseError::InvalidIndex("data memory", memory_index as u32),
                                )?
                            }
                            _ => return Err(ParseError::InvalidSection),
                        };

                        module.data.push(DataSegment {
                            memory_index,
                            offset: offset.ok_or(ParseError::UnexpectedEof)?,
                            data: data.data.to_vec(),
                        });
                    }
                }
                _ => {}
            }
        }

        // Add types to module
        for (params, results) in &self.types {
            let params_type = Self::types_to_type(params);
            let results_type = Self::types_to_type(results);
            module.types.push(crate::ir::module::FuncType {
                params: params_type,
                results: results_type,
            });
        }

        // Combine function signatures with code
        let num_func_imports = self.func_ids.len() - function_section.len();
        for (i, (locals, expr)) in code_section.into_iter().enumerate() {
            let func_id = self.func_ids[num_func_imports + i];
            let func = module.get_function_mut(func_id).unwrap();
            func.vars = locals;
            func.body = expr;
        }

        Ok(module)
    }

    fn parse_expression(
        &mut self,
        mut reader: OperatorsReader,
        module: &Module<'a>,
    ) -> Result<Option<ExprRef<'a>>> {
        self.parse_expression_impl(&mut reader, &mut Vec::new(), module)
    }

    fn parse_expression_impl(
        &mut self,
        reader: &mut OperatorsReader,
        label_stack: &mut Vec<Option<String>>,
        module: &Module<'a>,
    ) -> Result<Option<ExprRef<'a>>> {
        let builder = IrBuilder::new(self.bump);
        let mut stack: Vec<ExprRef<'a>> = Vec::new();

        while !reader.eof() {
            let op = reader.read()?;
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
                    let expr =
                        Expression::new(self.bump, ExpressionKind::Unreachable, Type::UNREACHABLE);
                    stack.push(expr);
                }
                Operator::Nop => {
                    stack.push(builder.nop());
                }
                Operator::Block { blockty } => {
                    let result_type = self.blocktype_to_type(blockty);
                    let name = self.make_label_name();
                    label_stack.push(Some(name.clone()));

                    if let Some(body) = self.parse_expression_impl(reader, label_stack, module)? {
                        label_stack.pop();
                        let block_expr = builder.block(
                            Some(self.bump.alloc_str(&name)),
                            BumpVec::from_iter_in([body], self.bump),
                            result_type,
                        );
                        stack.push(block_expr);
                    } else {
                        label_stack.pop();
                        let block_expr = builder.block(
                            Some(self.bump.alloc_str(&name)),
                            BumpVec::new_in(self.bump),
                            result_type,
                        );
                        stack.push(block_expr);
                    }
                }
                Operator::Loop { blockty } => {
                    let result_type = self.blocktype_to_type(blockty);
                    let name = self.make_label_name();
                    label_stack.push(Some(name.clone()));

                    if let Some(body) = self.parse_expression_impl(reader, label_stack, module)? {
                        label_stack.pop();
                        let loop_expr =
                            builder.loop_(Some(self.bump.alloc_str(&name)), body, result_type);
                        stack.push(loop_expr);
                    } else {
                        label_stack.pop();
                        return Err(ParseError::UnexpectedEof);
                    }
                }
                Operator::If { blockty } => {
                    let result_type = self.blocktype_to_type(blockty);
                    let condition = stack.pop().ok_or(ParseError::UnexpectedEof)?;

                    let name = self.make_label_name();
                    label_stack.push(Some(name.clone()));
                    let if_true = self
                        .parse_expression_impl(reader, label_stack, module)?
                        .ok_or(ParseError::UnexpectedEof)?;

                    let if_false = if self.last_op_was_else {
                        self.parse_expression_impl(reader, label_stack, module)?
                    } else {
                        None
                    };

                    label_stack.pop();
                    let if_expr = builder.if_(condition, if_true, if_false, result_type);

                    let if_wrapper = builder.block(
                        Some(self.bump.alloc_str(&name)),
                        BumpVec::from_iter_in([if_expr], self.bump),
                        result_type,
                    );
                    stack.push(if_wrapper);
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

                    let value = stack.pop();
                    let break_expr =
                        builder.break_(self.bump.alloc_str(label), None, value, Type::UNREACHABLE);
                    stack.push(break_expr);
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

                    let condition = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let value = stack.pop();
                    let break_expr = builder.break_(
                        self.bump.alloc_str(label),
                        Some(condition),
                        value,
                        Type::I32,
                    );
                    stack.push(break_expr);
                }
                Operator::BrTable { targets } => {
                    let mut names = BumpVec::with_capacity_in(targets.len() as usize, self.bump);
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
                        names.push(self.bump.alloc_str(label) as &str);
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
                    let default = self.bump.alloc_str(default) as &str;

                    let condition = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let value = if !stack.is_empty() {
                        Some(stack.pop().unwrap())
                    } else {
                        None
                    };

                    let switch_expr = builder.switch(names, default, condition, value);
                    stack.push(switch_expr);
                }
                Operator::Return => {
                    let value = stack.pop();
                    let return_expr = Expression::new(
                        self.bump,
                        ExpressionKind::Return { value },
                        Type::UNREACHABLE,
                    );
                    stack.push(return_expr);
                }
                Operator::Drop => {
                    let value = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let drop_expr =
                        Expression::new(self.bump, ExpressionKind::Drop { value }, Type::NONE);
                    stack.push(drop_expr);
                }
                Operator::Select => {
                    let condition = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let if_false = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let if_true = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let result_type = if_true.type_;
                    let select_expr = Expression::new(
                        self.bump,
                        ExpressionKind::Select {
                            condition,
                            if_true,
                            if_false,
                        },
                        result_type,
                    );
                    stack.push(select_expr);
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
                    let mut operands = BumpVec::with_capacity_in(param_count, self.bump);
                    for _ in 0..param_count {
                        operands.push(stack.pop().ok_or(ParseError::UnexpectedEof)?);
                    }
                    operands.reverse();

                    let func_id = self
                        .func_ids
                        .get(function_index as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("call func", function_index))?;

                    let result_type = Self::types_to_type(&func_type.1);

                    let call_expr = builder.call(func_id, operands, result_type, false);
                    stack.push(call_expr);
                }
                Operator::CallIndirect {
                    type_index,
                    table_index,
                } => {
                    let func_type = self
                        .types
                        .get(type_index as usize)
                        .ok_or(ParseError::InvalidType)?;
                    let param_count = func_type.0.len();

                    let params_type = Self::types_to_type(&func_type.0);
                    let result_type = Self::types_to_type(&func_type.1);
                    let sig_type =
                        crate::ir::type_store::intern_signature(params_type, result_type);

                    let target = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let mut operands = BumpVec::with_capacity_in(param_count, self.bump);
                    for _ in 0..param_count {
                        operands.push(stack.pop().ok_or(ParseError::UnexpectedEof)?);
                    }
                    operands.reverse();

                    let table_id = self
                        .table_ids
                        .get(table_index as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("call_indirect table", table_index))?;

                    let call_expr =
                        builder.call_indirect(table_id, target, operands, sig_type, result_type);
                    stack.push(call_expr);
                }
                Operator::LocalGet { local_index } => {
                    stack.push(builder.local_get(local_index, Type::I32)); // TODO: track actual type
                }
                Operator::LocalSet { local_index } => {
                    let value = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.local_set(local_index, value));
                }
                Operator::LocalTee { local_index } => {
                    let value = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let value_type = value.type_;
                    stack.push(builder.local_tee(local_index, value, value_type));
                }
                Operator::GlobalGet { global_index } => {
                    let id = self
                        .global_ids
                        .get(global_index as usize)
                        .copied()
                        .ok_or(ParseError::InvalidSection)?;
                    stack.push(builder.global_get(id, Type::I32)); // TODO: track actual type
                }
                Operator::GlobalSet { global_index } => {
                    let id = self
                        .global_ids
                        .get(global_index as usize)
                        .copied()
                        .ok_or(ParseError::InvalidSection)?;
                    let value = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.global_set(id, value));
                }
                Operator::I32Load { memarg } => {
                    let ptr = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let memory = self
                        .memory_ids
                        .get(0)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(builder.load(
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
                        .get(0)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(builder.load(
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
                        .get(0)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(builder.load(
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
                        .get(0)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(builder.load(
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
                        .get(0)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(builder.load(
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
                        .get(0)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(builder.load(
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
                        .get(0)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(builder.load(
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
                        .get(0)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(builder.load(
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
                        .get(0)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(builder.load(
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
                        .get(0)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(builder.load(
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
                        .get(0)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(builder.load(
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
                        .get(0)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(builder.load(
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
                        .get(0)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(builder.load(
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
                        .get(0)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(builder.load(
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
                        .get(0)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(builder.store(
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
                        .get(0)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(builder.store(
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
                        .get(0)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(builder.store(
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
                        .get(0)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(builder.store(
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
                        .get(0)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(builder.store(
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
                        .get(0)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(builder.store(
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
                        .get(0)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(builder.store(
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
                        .get(0)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(builder.store(
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
                        .get(0)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", 0))?;
                    stack.push(builder.store(
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
                        .ok_or(ParseError::InvalidIndex("memory", mem as u32))?;
                    stack.push(builder.memory_size(memory));
                }
                Operator::MemoryGrow { mem, .. } => {
                    let delta = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let memory = self
                        .memory_ids
                        .get(mem as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("memory", mem as u32))?;
                    stack.push(builder.memory_grow(memory, delta));
                }
                Operator::I32Const { value } => {
                    stack.push(builder.const_(Literal::I32(value)));
                }
                Operator::I64Const { value } => {
                    stack.push(builder.const_(Literal::I64(value)));
                }
                Operator::F32Const { value } => {
                    stack.push(builder.const_(Literal::F32(f32::from_bits(value.bits()))));
                }
                Operator::F64Const { value } => {
                    stack.push(builder.const_(Literal::F64(f64::from_bits(value.bits()))));
                }
                Operator::I32Add => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::AddInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32Sub => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::SubInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32Mul => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::MulInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32DivS => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::DivSInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32DivU => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::DivUInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32RemS => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::RemSInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32RemU => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::RemUInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32And => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::AndInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32Or => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::OrInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32Xor => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::XorInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32Shl => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::ShlInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32ShrS => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::ShrSInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32ShrU => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::ShrUInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32Rotl => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::RotLInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32Rotr => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::RotRInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I64Add => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::AddInt64,
                        left,
                        right,
                        Type::I64,
                    ));
                }
                Operator::I64Sub => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::SubInt64,
                        left,
                        right,
                        Type::I64,
                    ));
                }
                Operator::I64Mul => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::MulInt64,
                        left,
                        right,
                        Type::I64,
                    ));
                }
                Operator::I64DivS => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::DivSInt64,
                        left,
                        right,
                        Type::I64,
                    ));
                }
                Operator::I64DivU => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::DivUInt64,
                        left,
                        right,
                        Type::I64,
                    ));
                }
                Operator::I64RemS => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::RemSInt64,
                        left,
                        right,
                        Type::I64,
                    ));
                }
                Operator::I64RemU => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::RemUInt64,
                        left,
                        right,
                        Type::I64,
                    ));
                }
                Operator::I64And => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::AndInt64,
                        left,
                        right,
                        Type::I64,
                    ));
                }
                Operator::I64Or => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::OrInt64,
                        left,
                        right,
                        Type::I64,
                    ));
                }
                Operator::I64Xor => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::XorInt64,
                        left,
                        right,
                        Type::I64,
                    ));
                }
                Operator::I64Shl => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::ShlInt64,
                        left,
                        right,
                        Type::I64,
                    ));
                }
                Operator::I64ShrS => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::ShrSInt64,
                        left,
                        right,
                        Type::I64,
                    ));
                }
                Operator::I64ShrU => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::ShrUInt64,
                        left,
                        right,
                        Type::I64,
                    ));
                }
                Operator::I64Rotl => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::RotLInt64,
                        left,
                        right,
                        Type::I64,
                    ));
                }
                Operator::I64Rotr => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::RotRInt64,
                        left,
                        right,
                        Type::I64,
                    ));
                }
                Operator::I32Eq => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::EqInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32Ne => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::NeInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32LtS => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::LtSInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32LtU => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::LtUInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32GtS => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::GtSInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32GtU => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::GtUInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32LeS => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::LeSInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32LeU => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::LeUInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32GeS => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::GeSInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32GeU => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::GeUInt32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I64Eq => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::EqInt64,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I64Ne => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::NeInt64,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I64LtS => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::LtSInt64,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I64LtU => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::LtUInt64,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I64GtS => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::GtSInt64,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I64GtU => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::GtUInt64,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I64LeS => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::LeSInt64,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I64LeU => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::LeUInt64,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I64GeS => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::GeSInt64,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I64GeU => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::GeUInt64,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::F32Add => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::AddFloat32,
                        left,
                        right,
                        Type::F32,
                    ));
                }
                Operator::F32Sub => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::SubFloat32,
                        left,
                        right,
                        Type::F32,
                    ));
                }
                Operator::F32Mul => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::MulFloat32,
                        left,
                        right,
                        Type::F32,
                    ));
                }
                Operator::F32Div => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::DivFloat32,
                        left,
                        right,
                        Type::F32,
                    ));
                }
                Operator::F32Min => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::MinFloat32,
                        left,
                        right,
                        Type::F32,
                    ));
                }
                Operator::F32Max => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::MaxFloat32,
                        left,
                        right,
                        Type::F32,
                    ));
                }
                Operator::F32Copysign => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::CopySignFloat32,
                        left,
                        right,
                        Type::F32,
                    ));
                }
                Operator::F64Add => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::AddFloat64,
                        left,
                        right,
                        Type::F64,
                    ));
                }
                Operator::F64Sub => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::SubFloat64,
                        left,
                        right,
                        Type::F64,
                    ));
                }
                Operator::F64Mul => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::MulFloat64,
                        left,
                        right,
                        Type::F64,
                    ));
                }
                Operator::F64Div => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::DivFloat64,
                        left,
                        right,
                        Type::F64,
                    ));
                }
                Operator::F64Min => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::MinFloat64,
                        left,
                        right,
                        Type::F64,
                    ));
                }
                Operator::F64Max => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::MaxFloat64,
                        left,
                        right,
                        Type::F64,
                    ));
                }
                Operator::F64Copysign => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::CopySignFloat64,
                        left,
                        right,
                        Type::F64,
                    ));
                }
                Operator::F32Eq => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::EqFloat32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::F32Ne => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::NeFloat32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::F32Lt => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::LtFloat32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::F32Gt => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::GtFloat32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::F32Le => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::LeFloat32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::F32Ge => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::GeFloat32,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::F64Eq => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::EqFloat64,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::F64Ne => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::NeFloat64,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::F64Lt => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::LtFloat64,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::F64Gt => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::GtFloat64,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::F64Le => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::LeFloat64,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::F64Ge => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.binary(
                        crate::ir::ops::BinaryOp::GeFloat64,
                        left,
                        right,
                        Type::I32,
                    ));
                }
                Operator::I32Clz => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(crate::ir::ops::UnaryOp::ClzInt32, val, Type::I32));
                }
                Operator::I32Ctz => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(crate::ir::ops::UnaryOp::CtzInt32, val, Type::I32));
                }
                Operator::I32Popcnt => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(crate::ir::ops::UnaryOp::PopcntInt32, val, Type::I32));
                }
                Operator::I32Eqz => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(crate::ir::ops::UnaryOp::EqZInt32, val, Type::I32));
                }
                Operator::I64Clz => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(crate::ir::ops::UnaryOp::ClzInt64, val, Type::I64));
                }
                Operator::I64Ctz => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(crate::ir::ops::UnaryOp::CtzInt64, val, Type::I64));
                }
                Operator::I64Popcnt => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(crate::ir::ops::UnaryOp::PopcntInt64, val, Type::I64));
                }
                Operator::I64Eqz => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(crate::ir::ops::UnaryOp::EqZInt64, val, Type::I32));
                }
                Operator::F32Abs => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(crate::ir::ops::UnaryOp::AbsFloat32, val, Type::F32));
                }
                Operator::F32Neg => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(crate::ir::ops::UnaryOp::NegFloat32, val, Type::F32));
                }
                Operator::F32Ceil => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(crate::ir::ops::UnaryOp::CeilFloat32, val, Type::F32));
                }
                Operator::F32Floor => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(
                        crate::ir::ops::UnaryOp::FloorFloat32,
                        val,
                        Type::F32,
                    ));
                }
                Operator::F32Trunc => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(
                        crate::ir::ops::UnaryOp::TruncFloat32,
                        val,
                        Type::F32,
                    ));
                }
                Operator::F32Nearest => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(
                        crate::ir::ops::UnaryOp::NearestFloat32,
                        val,
                        Type::F32,
                    ));
                }
                Operator::F32Sqrt => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(crate::ir::ops::UnaryOp::SqrtFloat32, val, Type::F32));
                }
                Operator::F64Abs => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(crate::ir::ops::UnaryOp::AbsFloat64, val, Type::F64));
                }
                Operator::F64Neg => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(crate::ir::ops::UnaryOp::NegFloat64, val, Type::F64));
                }
                Operator::F64Ceil => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(crate::ir::ops::UnaryOp::CeilFloat64, val, Type::F64));
                }
                Operator::F64Floor => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(
                        crate::ir::ops::UnaryOp::FloorFloat64,
                        val,
                        Type::F64,
                    ));
                }
                Operator::F64Trunc => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(
                        crate::ir::ops::UnaryOp::TruncFloat64,
                        val,
                        Type::F64,
                    ));
                }
                Operator::F64Nearest => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(
                        crate::ir::ops::UnaryOp::NearestFloat64,
                        val,
                        Type::F64,
                    ));
                }
                Operator::F64Sqrt => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(crate::ir::ops::UnaryOp::SqrtFloat64, val, Type::F64));
                }
                Operator::I32WrapI64 => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(crate::ir::ops::UnaryOp::WrapInt64, val, Type::I32));
                }
                Operator::I64ExtendI32S => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(
                        crate::ir::ops::UnaryOp::ExtendSInt32,
                        val,
                        Type::I64,
                    ));
                }
                Operator::I64ExtendI32U => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(
                        crate::ir::ops::UnaryOp::ExtendUInt32,
                        val,
                        Type::I64,
                    ));
                }
                Operator::I32TruncF32S => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(
                        crate::ir::ops::UnaryOp::TruncSFloat32ToInt32,
                        val,
                        Type::I32,
                    ));
                }
                Operator::I32TruncF32U => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(
                        crate::ir::ops::UnaryOp::TruncUFloat32ToInt32,
                        val,
                        Type::I32,
                    ));
                }
                Operator::I32TruncF64S => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(
                        crate::ir::ops::UnaryOp::TruncSFloat64ToInt32,
                        val,
                        Type::I32,
                    ));
                }
                Operator::I32TruncF64U => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(
                        crate::ir::ops::UnaryOp::TruncUFloat64ToInt32,
                        val,
                        Type::I32,
                    ));
                }
                Operator::I64TruncF32S => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(
                        crate::ir::ops::UnaryOp::TruncSFloat32ToInt64,
                        val,
                        Type::I64,
                    ));
                }
                Operator::I64TruncF32U => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(
                        crate::ir::ops::UnaryOp::TruncUFloat32ToInt64,
                        val,
                        Type::I64,
                    ));
                }
                Operator::I64TruncF64S => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(
                        crate::ir::ops::UnaryOp::TruncSFloat64ToInt64,
                        val,
                        Type::I64,
                    ));
                }
                Operator::I64TruncF64U => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(
                        crate::ir::ops::UnaryOp::TruncUFloat64ToInt64,
                        val,
                        Type::I64,
                    ));
                }
                Operator::F32ConvertI32S => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(
                        crate::ir::ops::UnaryOp::ConvertSInt32ToFloat32,
                        val,
                        Type::F32,
                    ));
                }
                Operator::F32ConvertI32U => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(
                        crate::ir::ops::UnaryOp::ConvertUInt32ToFloat32,
                        val,
                        Type::F32,
                    ));
                }
                Operator::F32ConvertI64S => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(
                        crate::ir::ops::UnaryOp::ConvertSInt64ToFloat32,
                        val,
                        Type::F32,
                    ));
                }
                Operator::F32ConvertI64U => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(
                        crate::ir::ops::UnaryOp::ConvertUInt64ToFloat32,
                        val,
                        Type::F32,
                    ));
                }
                Operator::F64ConvertI32S => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(
                        crate::ir::ops::UnaryOp::ConvertSInt32ToFloat64,
                        val,
                        Type::F64,
                    ));
                }
                Operator::F64ConvertI32U => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(
                        crate::ir::ops::UnaryOp::ConvertUInt32ToFloat64,
                        val,
                        Type::F64,
                    ));
                }
                Operator::F64ConvertI64S => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(
                        crate::ir::ops::UnaryOp::ConvertSInt64ToFloat64,
                        val,
                        Type::F64,
                    ));
                }
                Operator::F64ConvertI64U => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(
                        crate::ir::ops::UnaryOp::ConvertUInt64ToFloat64,
                        val,
                        Type::F64,
                    ));
                }
                Operator::F32DemoteF64 => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(
                        crate::ir::ops::UnaryOp::DemoteFloat64,
                        val,
                        Type::F32,
                    ));
                }
                Operator::F64PromoteF32 => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(
                        crate::ir::ops::UnaryOp::PromoteFloat32,
                        val,
                        Type::F64,
                    ));
                }
                Operator::I32ReinterpretF32 => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(
                        crate::ir::ops::UnaryOp::ReinterpretFloat32,
                        val,
                        Type::I32,
                    ));
                }
                Operator::I64ReinterpretF64 => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(
                        crate::ir::ops::UnaryOp::ReinterpretFloat64,
                        val,
                        Type::I64,
                    ));
                }
                Operator::F32ReinterpretI32 => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(
                        crate::ir::ops::UnaryOp::ReinterpretInt32,
                        val,
                        Type::F32,
                    ));
                }
                Operator::F64ReinterpretI64 => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.unary(
                        crate::ir::ops::UnaryOp::ReinterpretInt64,
                        val,
                        Type::F64,
                    ));
                }
                Operator::DataDrop { data_index } => {
                    stack.push(builder.data_drop(data_index));
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
                    stack.push(builder.memory_init(memory, data_index, dest, src, size));
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
                    stack.push(builder.memory_copy(dst_id, src_id, dest, src, size));
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
                    stack.push(builder.memory_fill(memory, dest, value, size));
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
                    stack.push(builder.ref_null(ty));
                }
                Operator::RefIsNull => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.ref_is_null(val));
                }
                Operator::RefFunc { function_index } => {
                    let func_id = self
                        .func_ids
                        .get(function_index as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("function", function_index))?;
                    stack.push(builder.ref_func(func_id));
                }
                Operator::RefEq => {
                    let right = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let left = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.ref_eq(left, right));
                }
                Operator::TableGet { table } => {
                    let index = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let table_id = self
                        .table_ids
                        .get(table as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("table", table))?;
                    let ty = module.tables[table_id].limits.element_type;
                    stack.push(builder.table_get(table_id, index, ty));
                }
                Operator::TableSet { table } => {
                    let value = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let index = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let table_id = self
                        .table_ids
                        .get(table as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("table", table))?;
                    stack.push(builder.table_set(table_id, index, value));
                }
                Operator::TableSize { table } => {
                    let table_id = self
                        .table_ids
                        .get(table as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("table", table))?;
                    stack.push(builder.table_size(table_id));
                }
                Operator::TableGrow { table } => {
                    let delta = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let value = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let table_id = self
                        .table_ids
                        .get(table as usize)
                        .copied()
                        .ok_or(ParseError::InvalidIndex("table", table))?;
                    stack.push(builder.table_grow(table_id, value, delta));
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
                    stack.push(builder.table_fill(table_id, dest, value, size));
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
                    stack.push(builder.table_copy(dst_id, src_id, dest, src, size));
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
                    stack.push(builder.table_init(table_id, elem_index, dest, offset, size));
                }
                Operator::ElemDrop { elem_index } => {
                    stack.push(builder.elem_drop(elem_index));
                }
                Operator::RefI31 => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.i31_new(val));
                }
                Operator::I31GetS => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.i31_get(val, true));
                }
                Operator::I31GetU => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.i31_get(val, false));
                }
                Operator::StructNew { struct_type_index } => {
                    let ty = self
                        .types
                        .get(struct_type_index as usize)
                        .cloned()
                        .unwrap_or((vec![], vec![]));
                    let param_count = ty.0.len();
                    let mut operands = BumpVec::with_capacity_in(param_count, self.bump);
                    for _ in 0..param_count {
                        operands.push(stack.pop().ok_or(ParseError::UnexpectedEof)?);
                    }
                    operands.reverse();
                    stack.push(builder.struct_new(Type::new(HeapType::STRUCT, true), operands));
                    // TODO: refine type
                }
                Operator::StructGet {
                    struct_type_index: _,
                    field_index,
                } => {
                    let ptr = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.struct_get(
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
                    stack.push(builder.struct_set(
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
                    stack.push(builder.array_new(Type::new(HeapType::ARRAY, true), size, None));
                }
                Operator::ArrayGet {
                    array_type_index: _,
                } => {
                    let index = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    let ptr = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.array_get(
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
                    stack.push(builder.array_set(
                        Type::new(HeapType::ARRAY, true),
                        ptr,
                        index,
                        value,
                    ));
                }
                Operator::ArrayLen => {
                    let ptr = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.array_len(ptr));
                }
                Operator::RefTestNonNull { hty: _ } | Operator::RefTestNullable { hty: _ } => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.ref_test(crate::ir::ops::RefTestOp::Ref, val, Type::ANYREF));
                    // TODO
                }
                Operator::RefCastNonNull { hty: _ } | Operator::RefCastNullable { hty: _ } => {
                    let val = stack.pop().ok_or(ParseError::UnexpectedEof)?;
                    stack.push(builder.ref_cast(
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

        self.last_op_was_else = false;

        if stack.is_empty() {
            Ok(Some(builder.nop()))
        } else if stack.len() > 1 {
            let list = BumpVec::from_iter_in(stack, self.bump);
            let result_type = list.last().map(|e| e.type_).unwrap_or(Type::NONE);
            Ok(Some(builder.block(None, list, result_type)))
        } else {
            Ok(stack.pop())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal_module() {
        let bump = Bump::new();

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

        let mut reader = BinaryReader::new(&bump, wasm);
        let module = reader.parse_module().expect("Failed to parse module");

        assert_eq!(module.functions.len(), 1);
        let func = &module.functions.iter().next().unwrap().1;
        assert_eq!(func.results, Type::I32);

        if let Some(body) = func.body() {
            match &body.kind {
                ExpressionKind::Const(Literal::I32(42)) => {} // Success
                _ => panic!("Expected i32.const 42, got {:?}", body.kind),
            }
        } else {
            panic!("Function body is None");
        }
    }

    #[test]
    fn test_parse_multi_param_function() {
        let bump = Bump::new();

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

        let mut reader = BinaryReader::new(&bump, wasm);
        let module = reader.parse_module().expect("Failed to parse module");

        assert_eq!(module.functions.len(), 1);
        let func = &module.functions.iter().next().unwrap().1;

        let expected_params = crate::ir::type_store::intern_tuple(vec![Type::I32, Type::I32]);
        assert_eq!(func.params, expected_params);
        assert_eq!(func.results, Type::I32);
    }
}
