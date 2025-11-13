use crate::log::debug;
use crate::{ParseError, Result, conversion};
use alloc::string::ToString;
use alloc::{boxed::Box, format, vec::Vec};
use tinywasm_types::{
    Data, Element, Export, FuncType, Global, Import, Instruction, MemoryType, TableType, TinyWasmModule, ValueCounts,
    ValueCountsSmall, WasmFunction, WasmFunctionData,
};
use wasmparser::{FuncValidatorAllocations, Payload, Validator};

pub(crate) type Code = (Box<[Instruction]>, WasmFunctionData, ValueCounts);

#[derive(Default)]
pub(crate) struct ModuleReader {
    func_validator_allocations: Option<FuncValidatorAllocations>,

    pub(crate) version: Option<u16>,
    pub(crate) start_func: Option<u32>,
    pub(crate) func_types: Vec<FuncType>,
    pub(crate) code_type_addrs: Vec<u32>,
    pub(crate) exports: Vec<Export>,
    pub(crate) code: Vec<Code>,
    pub(crate) globals: Vec<Global>,
    pub(crate) table_types: Vec<TableType>,
    pub(crate) memory_types: Vec<MemoryType>,
    #[cfg(feature = "stack-switching")]
    pub(crate) tags: Vec<crate::conversion::TagType>,
    pub(crate) imports: Vec<Import>,
    pub(crate) data: Vec<Data>,
    pub(crate) elements: Vec<Element>,
    pub(crate) end_reached: bool,
}

impl ModuleReader {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn process_payload(&mut self, payload: Payload<'_>, validator: &mut Validator) -> Result<()> {
        use wasmparser::Payload::*;

        match payload {
            Version { num, encoding, range } => {
                validator.version(num, encoding, &range)?;
                self.version = Some(num);
                match encoding {
                    wasmparser::Encoding::Module => {}
                    wasmparser::Encoding::Component => return Err(ParseError::InvalidEncoding(encoding)),
                }
            }
            StartSection { func, range } => {
                if self.start_func.is_some() {
                    return Err(ParseError::DuplicateSection("Start section".into()));
                }

                debug!("Found start section");
                validator.start_section(func, &range)?;
                self.start_func = Some(func);
            }
            TypeSection(reader) => {
                if !self.func_types.is_empty() {
                    return Err(ParseError::DuplicateSection("Type section".into()));
                }

                debug!("Found type section");
                validator.type_section(&reader)?;
                self.func_types = reader
                    .into_iter()
                    .map(|t| conversion::convert_module_type(t?))
                    .collect::<Result<Vec<FuncType>>>()?;
            }

            GlobalSection(reader) => {
                if !self.globals.is_empty() {
                    return Err(ParseError::DuplicateSection("Global section".into()));
                }

                debug!("Found global section");
                validator.global_section(&reader)?;
                self.globals = conversion::convert_module_globals(reader)?;
            }
            TableSection(reader) => {
                if !self.table_types.is_empty() {
                    return Err(ParseError::DuplicateSection("Table section".into()));
                }
                debug!("Found table section");
                validator.table_section(&reader)?;
                self.table_types = conversion::convert_module_tables(reader)?;
            }
            MemorySection(reader) => {
                if !self.memory_types.is_empty() {
                    return Err(ParseError::DuplicateSection("Memory section".into()));
                }

                debug!("Found memory section");
                validator.memory_section(&reader)?;
                self.memory_types = conversion::convert_module_memories(reader)?;
            }
            ElementSection(reader) => {
                debug!("Found element section");
                validator.element_section(&reader)?;
                self.elements = conversion::convert_module_elements(reader)?;
            }
            DataSection(reader) => {
                if !self.data.is_empty() {
                    return Err(ParseError::DuplicateSection("Data section".into()));
                }

                debug!("Found data section");
                validator.data_section(&reader)?;
                self.data = conversion::convert_module_data_sections(reader)?;
            }
            DataCountSection { count, range } => {
                debug!("Found data count section");
                if !self.data.is_empty() {
                    return Err(ParseError::DuplicateSection("Data count section".into()));
                }
                validator.data_count_section(count, &range)?;
            }
            FunctionSection(reader) => {
                if !self.code_type_addrs.is_empty() {
                    return Err(ParseError::DuplicateSection("Function section".into()));
                }

                debug!("Found function section");
                validator.function_section(&reader)?;
                self.code_type_addrs = reader.into_iter().map(|f| Ok(f?)).collect::<Result<Vec<_>>>()?;
            }
            CodeSectionStart { count, range, .. } => {
                debug!("Found code section ({count} functions)");
                if !self.code.is_empty() {
                    return Err(ParseError::DuplicateSection("Code section".into()));
                }
                self.code.reserve(count as usize);
                validator.code_section_start(&range)?;
            }
            CodeSectionEntry(function) => {
                debug!("Found code section entry");
                let v = validator.code_section_entry(&function)?;
                let func_validator = v.into_validator(self.func_validator_allocations.take().unwrap_or_default());
                let (code, allocations) = conversion::convert_module_code(function, func_validator)?;
                self.code.push(code);
                self.func_validator_allocations = Some(allocations);
            }
            ImportSection(reader) => {
                if !self.imports.is_empty() {
                    return Err(ParseError::DuplicateSection("Import section".into()));
                }

                debug!("Found import section");
                validator.import_section(&reader)?;
                self.imports = conversion::convert_module_imports(reader)?;
            }
            ExportSection(reader) => {
                if !self.exports.is_empty() {
                    return Err(ParseError::DuplicateSection("Export section".into()));
                }

                debug!("Found export section");
                validator.export_section(&reader)?;
                self.exports =
                    reader.into_iter().map(|e| conversion::convert_module_export(e?)).collect::<Result<Vec<_>>>()?;
            }
            #[cfg(feature = "stack-switching")]
            TagSection(reader) => {
                debug!("Found tag section");
                validator.tag_section(&reader)?;
                self.tags = conversion::convert_module_tags(reader, &self.func_types)?;
            }
            End(offset) => {
                debug!("Reached end of module");
                if self.end_reached {
                    return Err(ParseError::DuplicateSection("End section".into()));
                }

                validator.end(offset)?;
                self.end_reached = true;
            }
            CustomSection(_reader) => {
                debug!("Found custom section");
                debug!("Skipping custom section: {:?}", _reader.name());
            }
            UnknownSection { .. } => return Err(ParseError::UnsupportedSection("Unknown section".into())),
            section => return Err(ParseError::UnsupportedSection(format!("Unsupported section: {section:?}"))),
        };

        Ok(())
    }

    pub(crate) fn into_module(self) -> Result<TinyWasmModule> {
        if !self.end_reached {
            return Err(ParseError::EndNotReached);
        }

        if self.code_type_addrs.len() != self.code.len() {
            return Err(ParseError::Other("Code and code type address count mismatch".to_string()));
        }

        // Basic post-parse validation for stack-switching instructions and tag indices.
        #[cfg(feature = "stack-switching")]
        {
            // Validate tag and resume table indices in parsed instructions.
            for (f_idx, ((instructions, data, _locals), _ty_idx)) in self.code.iter().zip(self.code_type_addrs).enumerate() {
                for instr in instructions.iter() {
                    use tinywasm_types::Instruction::*;
                    match instr {
                        Suspend(tag_idx) => {
                            let t = *tag_idx as usize;
                            if t >= self.tags.len() {
                                return Err(ParseError::UnsupportedOperator(format!("suspend: invalid tag index {} in function {}", t, f_idx)));
                            }
                        }
                        Resume(_ty, resume_idx) => {
                            let ri = *resume_idx as usize;
                            if ri >= data.resume_tables.len() {
                                return Err(ParseError::UnsupportedOperator(format!("resume: invalid resume_table index {} in function {}", ri, f_idx)));
                            }
                        }
                        ResumeThrow(_ty, tag_idx, resume_idx) => {
                            let t = *tag_idx as usize;
                            if t >= self.tags.len() {
                                return Err(ParseError::UnsupportedOperator(format!("resume.throw: invalid tag index {} in function {}", t, f_idx)));
                            }
                            let ri = *resume_idx as usize;
                            if ri >= data.resume_tables.len() {
                                return Err(ParseError::UnsupportedOperator(format!("resume.throw: invalid resume_table index {} in function {}", ri, f_idx)));
                            }
                        }
                        Switch(_ty, tag_idx) => {
                            let t = *tag_idx as usize;
                            if t >= self.tags.len() {
                                return Err(ParseError::UnsupportedOperator(format!("switch: invalid tag index {} in function {}", t, f_idx)));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        let funcs = self
            .code
            .into_iter()
            .zip(self.code_type_addrs)
            .map(|((instructions, data, locals), ty_idx)| {
                let ty = self.func_types.get(ty_idx as usize).expect("No func type for func, this is a bug").clone();
                let params = ValueCountsSmall::from(&ty.params);
                WasmFunction { instructions, data, locals, params, ty }
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();

        let globals = self.globals;
        let table_types = self.table_types;

        Ok(TinyWasmModule {
            funcs,
            func_types: self.func_types.into_boxed_slice(),
            globals: globals.into_boxed_slice(),
            table_types: table_types.into_boxed_slice(),
            imports: self.imports.into_boxed_slice(),
            start_func: self.start_func,
            data: self.data.into_boxed_slice(),
            exports: self.exports.into_boxed_slice(),
            elements: self.elements.into_boxed_slice(),
            memory_types: self.memory_types.into_boxed_slice(),
            #[cfg(feature = "stack-switching")]
            tags: self.tags.into_boxed_slice(),
        })
    }
}
