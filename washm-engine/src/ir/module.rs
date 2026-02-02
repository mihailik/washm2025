use crate::collections::id_arena::{Id, IdArena};
use crate::ir::annotation::AnnotationStore;
use crate::ir::expression::ExprRef;
use crate::ir::wasm_features::FeatureSet;
use crate::ir::Type;

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub struct TypeMarker;
pub type TypeId = Id<TypeMarker>;

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub struct DataMarker;
pub type DataId = Id<DataMarker>;

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub struct ElementMarker;
pub type ElementId = Id<ElementMarker>;

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub struct GlobalMarker;
pub type GlobalId = Id<GlobalMarker>;

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub struct FuncMarker;
pub type FuncId = Id<FuncMarker>;

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub struct TableMarker;
pub type TableId = Id<TableMarker>;

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub struct MemoryMarker;
pub type MemoryId = Id<MemoryMarker>;

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub struct LocalMarker;
pub type LocalId = Id<LocalMarker>;

#[derive(Debug, Clone)]
pub struct Local {
    pub name: String,
    pub type_: Type,
}

#[derive(Debug, Clone)]
pub struct FunctionImport {
    pub module: String,
    pub name: String,
}

#[derive(Debug)]
pub enum FunctionKind<'a> {
    Local {
        locals: IdArena<LocalMarker, Local>,
        param_ids: Vec<LocalId>,
        body: ExprRef<'a>,
    },
    Import(FunctionImport),
}

#[derive(Debug)]
pub struct Function<'a> {
    pub name: String,
    pub type_idx: Option<TypeId>, // Pointer to module.types
    pub params: Type,
    pub results: Type,
    pub kind: FunctionKind<'a>,
}

impl<'a> Function<'a> {
    pub fn new(
        name: String,
        params: Type,
        results: Type,
        vars: Vec<Type>,
        body: Option<ExprRef<'a>>,
    ) -> Self {
        let mut locals: IdArena<LocalMarker, Local> = IdArena::new();
        let mut param_ids = Vec::new();
        for param_type in params.tuple_elements() {
            param_ids.push(
                locals
                    .alloc(Local {
                        name: String::new(),
                        type_: param_type,
                    })
                    .cast(),
            );
        }
        for var_type in vars {
            locals.alloc(Local {
                name: String::new(),
                type_: var_type,
            });
        }
        Self {
            name,
            params,
            results,
            type_idx: None,
            kind: FunctionKind::Local {
                locals,
                param_ids,
                body: body.unwrap_or_else(|| ExprRef::new_null()),
            },
        }
    }

    pub fn with_type_idx(
        name: String,
        type_idx: TypeId,
        params: Type,
        results: Type,
        vars: Vec<Type>,
        body: Option<ExprRef<'a>>,
    ) -> Self {
        let mut locals: IdArena<LocalMarker, Local> = IdArena::new();
        let mut param_ids = Vec::new();
        for param_type in params.tuple_elements() {
            param_ids.push(
                locals
                    .alloc(Local {
                        name: String::new(),
                        type_: param_type,
                    })
                    .cast(),
            );
        }
        for var_type in vars {
            locals.alloc(Local {
                name: String::new(),
                type_: var_type,
            });
        }
        Self {
            name,
            params,
            results,
            type_idx: Some(type_idx),
            kind: FunctionKind::Local {
                locals,
                param_ids,
                body: body.unwrap_or_else(|| ExprRef::new_null()),
            },
        }
    }

    pub fn new_import(
        name: String,
        module: String,
        field: String,
        params: Type,
        results: Type,
    ) -> Self {
        Self {
            name,
            params,
            results,
            type_idx: None,
            kind: FunctionKind::Import(FunctionImport {
                module,
                name: field,
            }),
        }
    }

    pub fn num_params(&self) -> usize {
        match &self.kind {
            FunctionKind::Local { param_ids, .. } => param_ids.len(),
            FunctionKind::Import(_) => self.params.tuple_elements().len(),
        }
    }

    pub fn param_ids(&self) -> &[LocalId] {
        match &self.kind {
            FunctionKind::Local { param_ids, .. } => param_ids,
            FunctionKind::Import(_) => &[],
        }
    }

    pub fn param_ids_mut(&mut self) -> Option<&mut Vec<LocalId>> {
        match &mut self.kind {
            FunctionKind::Local { param_ids, .. } => Some(param_ids),
            FunctionKind::Import(_) => None,
        }
    }

    pub fn body(&self) -> Option<ExprRef<'a>> {
        match &self.kind {
            FunctionKind::Local { body, .. } => Some(*body),
            FunctionKind::Import(_) => None,
        }
    }

    pub fn body_mut(&mut self) -> Option<&mut ExprRef<'a>> {
        match &mut self.kind {
            FunctionKind::Local { body, .. } => Some(body),
            FunctionKind::Import(_) => None,
        }
    }

    pub fn locals(&self) -> &IdArena<LocalMarker, Local> {
        match &self.kind {
            FunctionKind::Local { locals, .. } => locals,
            FunctionKind::Import(_) => panic!("Cannot get locals of an import"),
        }
    }

    pub fn locals_mut(&mut self) -> &mut IdArena<LocalMarker, Local> {
        match &mut self.kind {
            FunctionKind::Local { locals, .. } => locals,
            FunctionKind::Import(_) => panic!("Cannot get locals of an import"),
        }
    }

    pub fn is_import(&self) -> bool {
        matches!(self.kind, FunctionKind::Import(_))
    }

    #[cfg(test)]
    pub fn new_for_test(f: Function<'a>) -> Self {
        f
    }

    pub fn add_var(&mut self, type_: Type) -> LocalId {
        self.locals_mut()
            .alloc(Local {
                name: String::new(),
                type_,
            })
            .cast()
    }

    pub fn get_local_type(&self, id: LocalId) -> Type {
        self.locals()
            .get(id.cast())
            .map(|l| l.type_)
            .unwrap_or(Type::NONE)
    }

    pub fn vars(&self) -> Vec<Type> {
        let param_set: std::collections::HashSet<LocalId> =
            self.param_ids().iter().cloned().collect();
        self.locals()
            .iter()
            .filter(|&(id, _)| !param_set.contains(&id.cast()))
            .map(|(_, l)| l.type_)
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemoryLimits {
    pub initial: u32,         // Initial size in 64KB pages
    pub maximum: Option<u32>, // Optional maximum size
}

#[derive(Debug)]
pub struct Memory {
    pub name: String,
    pub limits: MemoryLimits,
    pub kind: MemoryKind,
}

#[derive(Debug, Clone)]
pub enum MemoryKind {
    Local,
    Import { module: String, field: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct TableLimits {
    pub element_type: Type,   // Element type (funcref in MVP)
    pub initial: u32,         // Initial size
    pub maximum: Option<u32>, // Optional maximum size
}

#[derive(Debug)]
pub struct Table {
    pub name: String,
    pub limits: TableLimits,
    pub kind: TableKind,
}

#[derive(Debug, Clone)]
pub enum TableKind {
    Import { module: String, field: String },
    Local,
}

#[derive(Debug)]
pub struct Global<'a> {
    pub name: String,
    pub type_: Type,
    pub mutable: bool,
    pub kind: GlobalKind<'a>,
}

#[derive(Debug)]
pub enum GlobalKind<'a> {
    Import { module: String, field: String },
    Local { init: ExprRef<'a> },
}

impl<'a> Global<'a> {
    pub fn is_import(&self) -> bool {
        matches!(self.kind, GlobalKind::Import { .. })
    }

    pub fn kind_to_init(&self) -> Option<ExprRef<'a>> {
        match self.kind {
            GlobalKind::Local { init } => Some(init),
            GlobalKind::Import { .. } => None,
        }
    }
}

impl<'a> GlobalKind<'a> {
    pub fn set_init(&mut self, new_init: ExprRef<'a>) {
        if let GlobalKind::Local { init } = self {
            *init = new_init;
        }
    }
}

#[derive(Debug, Clone)]
pub enum ImportKind {
    Function(Type, Type),
    Table(TableLimits),
    Memory(MemoryLimits),
    Global(Type, bool),
}

#[derive(Debug, Clone)]
pub struct Import {
    pub module: String,
    pub name: String,
    pub kind: ImportKind,
}

#[derive(Debug, Clone)]
pub struct Export {
    pub name: String,
    pub kind: ExportKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportKind {
    Function(FuncId),
    Table(TableId),
    Memory(MemoryId),
    Global(GlobalId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuncType {
    pub params: Type,  // Parameter types
    pub results: Type, // Result types
}

#[derive(Debug)]
pub struct DataSegment<'a> {
    pub memory_index: MemoryId,
    pub offset: ExprRef<'a>, // Offset expression (where in memory)
    pub data: Vec<u8>,       // The actual data bytes
}

#[derive(Debug)]
pub struct ElementSegment<'a> {
    pub table_index: TableId,
    pub offset: ExprRef<'a>,       // Offset expression (where in table)
    pub func_indices: Vec<FuncId>, // Function indices to initialize
}

#[derive(Debug)]
pub struct Module<'a> {
    pub allocator: &'a bumpalo::Bump,
    pub features: FeatureSet,
    pub types: IdArena<TypeMarker, FuncType>, // Type section
    pub functions: IdArena<FuncMarker, Function<'a>>,
    pub tables: IdArena<TableMarker, Table>,
    pub memories: IdArena<MemoryMarker, Memory>,
    pub globals: IdArena<GlobalMarker, Global<'a>>,
    pub start: Option<FuncId>, // Start function index
    pub exports: Vec<Export>,
    pub elements: IdArena<ElementMarker, ElementSegment<'a>>, // Element section
    pub data: IdArena<DataMarker, DataSegment<'a>>,
    pub type_names: Vec<String>, // Names for types (debug)
    pub annotations: AnnotationStore<'a>,
}

impl<'a> Module<'a> {
    pub fn new(allocator: &'a bumpalo::Bump) -> Self {
        Self {
            allocator,
            features: FeatureSet::DEFAULT,
            types: IdArena::new(),
            functions: IdArena::new(),
            tables: IdArena::new(),
            memories: IdArena::new(),
            globals: IdArena::new(),
            start: None,
            exports: Vec::new(),
            elements: IdArena::new(),
            data: IdArena::new(),
            type_names: Vec::new(),
            annotations: AnnotationStore::new(),
        }
    }

    pub fn allocator(&self) -> &'a bumpalo::Bump {
        self.allocator
    }

    pub fn add_function(&mut self, func: Function<'a>) -> FuncId {
        self.functions.alloc(func).cast()
    }

    pub fn get_function(&self, id: FuncId) -> Option<&Function<'a>> {
        self.functions.get(id.cast())
    }

    pub fn get_function_mut(&mut self, id: FuncId) -> Option<&mut Function<'a>> {
        self.functions.get_mut(id.cast())
    }

    pub fn get_function_by_name(&self, name: &str) -> Option<(FuncId, &Function<'a>)> {
        self.functions
            .iter()
            .find(|(_, f)| f.name == name)
            .map(|(id, f)| (id.cast(), f))
    }

    pub fn add_table(&mut self, table: Table) -> TableId {
        self.tables.alloc(table).cast()
    }

    pub fn set_table(&mut self, initial: u32, maximum: Option<u32>) {
        self.tables.alloc(Table {
            name: String::new(),
            limits: TableLimits {
                element_type: Type::FUNCREF,
                initial,
                maximum,
            },
            kind: TableKind::Local,
        });
    }

    pub fn get_table(&self, id: TableId) -> Option<&Table> {
        self.tables.get(id.cast())
    }

    pub fn add_memory(&mut self, memory: Memory) -> MemoryId {
        self.memories.alloc(memory).cast()
    }

    pub fn get_memory(&self, id: MemoryId) -> Option<&Memory> {
        self.memories.get(id.cast())
    }

    pub fn set_memory(&mut self, pages: u32, max: Option<u32>) {
        self.memories.alloc(Memory {
            name: String::new(),
            limits: MemoryLimits {
                initial: pages,
                maximum: max,
            },
            kind: MemoryKind::Local,
        });
    }

    pub fn add_import(&mut self, import: Import) {
        match &import.kind {
            ImportKind::Function(params, results) => {
                self.functions.alloc(Function::new_import(
                    import.name.clone(),
                    import.module.clone(),
                    import.name.clone(),
                    *params,
                    *results,
                ));
            }
            ImportKind::Table(limits) => {
                self.tables.alloc(Table {
                    name: import.name.clone(),
                    limits: limits.clone(),
                    kind: TableKind::Import {
                        module: import.module.clone(),
                        field: import.name.clone(),
                    },
                });
            }
            ImportKind::Memory(limits) => {
                self.memories.alloc(Memory {
                    name: import.name.clone(),
                    limits: limits.clone(),
                    kind: MemoryKind::Import {
                        module: import.module.clone(),
                        field: import.name.clone(),
                    },
                });
            }
            ImportKind::Global(type_, mutable) => {
                self.globals.alloc(Global {
                    name: import.name.clone(),
                    type_: *type_,
                    mutable: *mutable,
                    kind: GlobalKind::Import {
                        module: import.module.clone(),
                        field: import.name.clone(),
                    },
                });
            }
        }
    }

    pub fn add_export(&mut self, name: String, kind: ExportKind) {
        self.exports.push(Export { name, kind });
    }

    pub fn export_function(&mut self, id: FuncId, export_name: String) {
        self.add_export(export_name, ExportKind::Function(id));
    }

    pub fn add_global(&mut self, global: Global<'a>) -> GlobalId {
        self.globals.alloc(global).cast()
    }

    pub fn get_global(&self, id: GlobalId) -> Option<&Global<'a>> {
        self.globals.get(id.cast())
    }

    pub fn get_global_by_name(&self, name: &str) -> Option<(GlobalId, &Global<'a>)> {
        self.globals
            .iter()
            .find(|(_, g)| g.name == name)
            .map(|(id, g)| (id.cast(), g))
    }

    pub fn export_global(&mut self, id: GlobalId, export_name: String) {
        self.add_export(export_name, ExportKind::Global(id));
    }

    pub fn add_data_segment(&mut self, segment: DataSegment<'a>) -> DataId {
        self.data.alloc(segment).cast()
    }

    pub fn set_start(&mut self, id: FuncId) {
        self.start = Some(id);
    }

    pub fn add_element_segment(&mut self, segment: ElementSegment<'a>) -> ElementId {
        self.elements.alloc(segment).cast()
    }

    pub fn add_type(&mut self, params: Type, results: Type) -> TypeId {
        self.types.alloc(FuncType { params, results }).cast()
    }

    /// Finds the index of a signature type in the module's type section.
    pub fn find_type_id(&self, ty: Type) -> Option<TypeId> {
        let sig = crate::ir::type_store::lookup_signature(ty)?;
        self.types
            .iter()
            .find(|(_, t)| t.params == sig.params && t.results == sig.results)
            .map(|(id, _)| id.cast())
    }

    pub fn set_annotation(
        &mut self,
        expr: ExprRef<'a>,
        annotation: crate::ir::annotation::Annotation<'a>,
    ) {
        self.annotations.insert(expr, annotation);
    }

    pub fn get_annotations(
        &self,
        expr: ExprRef<'a>,
    ) -> Option<crate::ir::annotation::Annotations<'a>> {
        self.annotations.get(expr)
    }

    pub fn set_function_local_name(&mut self, _func_idx: usize, _local_idx: u32, _name: &'a str) {
        // Implementation pending schema decision
    }

    /// Read a WebAssembly module from binary format.
    pub fn read_binary(allocator: &'a bumpalo::Bump, binary: &[u8]) -> Result<Self, String> {
        let mut reader = crate::wasm::binary_reader::BinaryReader::new(allocator, binary.to_vec());
        reader
            .parse_module()
            .map_err(|e| format!("Binary parsing error: {:?}", e))
    }

    /// Read a WebAssembly module from WAT format using the "binary bridge".
    pub fn read_wat(allocator: &'a bumpalo::Bump, wat: &str) -> Result<Self, String> {
        let binary = wat::parse_str(wat).map_err(|e| format!("WAT parsing error: {}", e))?;
        let mut reader = crate::wasm::binary_reader::BinaryReader::new(allocator, binary);
        reader
            .parse_module()
            .map_err(|e| format!("Binary parsing error: {:?}", e))
    }

    /// Convert the module to WAT format using the "binary bridge".
    pub fn to_wat(&self) -> Result<String, String> {
        let mut writer = crate::wasm::binary_writer::BinaryWriter::new();
        let binary = writer
            .write_module(self)
            .map_err(|e| format!("Binary writing error: {:?}", e))?;
        wasmprinter::print_bytes(&binary).map_err(|e| format!("WAT printing error: {}", e))
    }
}
