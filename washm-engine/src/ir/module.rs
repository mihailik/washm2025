use crate::collections::id_arena::{Id, IdArena};
use crate::ir::annotation::AnnotationStore;
use crate::ir::expression::ExprRef;
use crate::ir::wasm_features::FeatureSet;
use crate::ir::Type;

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
pub struct FunctionImport {
    pub module: String,
    pub name: String,
}

#[derive(Debug)]
pub struct Function<'a> {
    pub name: String,
    pub type_idx: Option<u32>, // Index into module.types (None means infer from params/results)
    pub params: Type,
    pub results: Type,
    pub vars: Vec<Type>,
    pub body: Option<ExprRef<'a>>,
    pub local_names: Vec<String>,
    pub import: Option<FunctionImport>,
}

impl<'a> Function<'a> {
    pub fn new(
        name: String,
        params: Type,
        results: Type,
        vars: Vec<Type>,
        body: Option<ExprRef<'a>>,
    ) -> Self {
        let local_names = vec![String::new(); params.tuple_len() + vars.len()];
        Self {
            name,
            params,
            results,
            vars,
            body,
            type_idx: None,
            local_names,
            import: None,
        }
    }

    pub fn with_type_idx(
        name: String,
        type_idx: u32,
        params: Type,
        results: Type,
        vars: Vec<Type>,
        body: Option<ExprRef<'a>>,
    ) -> Self {
        let local_names = vec![String::new(); params.tuple_len() + vars.len()];
        Self {
            name,
            params,
            results,
            vars,
            body,
            type_idx: Some(type_idx),
            local_names,
            import: None,
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
            vars: Vec::new(),
            body: None,
            type_idx: None,
            local_names: Vec::new(),
            import: Some(FunctionImport {
                module,
                name: field,
            }),
        }
    }

    pub fn body(&self) -> Option<ExprRef<'a>> {
        self.body
    }

    pub fn body_mut(&mut self) -> &mut Option<ExprRef<'a>> {
        &mut self.body
    }

    pub fn vars(&self) -> &[Type] {
        &self.vars
    }

    pub fn vars_mut(&mut self) -> &mut Vec<Type> {
        &mut self.vars
    }

    pub fn local_names(&self) -> &[String] {
        &self.local_names
    }

    pub fn local_names_mut(&mut self) -> &mut Vec<String> {
        &mut self.local_names
    }

    pub fn is_import(&self) -> bool {
        self.import.is_some()
    }

    #[cfg(test)]
    pub fn new_for_test(mut f: Function<'a>) -> Self {
        f.import = None;
        f
    }

    pub fn add_var(&mut self, type_: Type) -> u32 {
        let params_len = self.params.tuple_len();
        let index = (params_len + self.vars.len()) as u32;
        self.vars.push(type_);
        self.local_names.push(String::new());
        index
    }

    pub fn get_local_type(&self, index: u32) -> Type {
        let params_len = self.params.tuple_len();
        if (index as usize) < params_len {
            self.params.tuple_elements()[index as usize]
        } else {
            self.vars[(index as usize) - params_len]
        }
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
    pub types: Vec<FuncType>, // Type section
    pub functions: IdArena<FuncMarker, Function<'a>>,
    pub tables: IdArena<TableMarker, Table>,
    pub memories: IdArena<MemoryMarker, Memory>,
    pub globals: IdArena<GlobalMarker, Global<'a>>,
    pub imports: Vec<Import>,
    pub start: Option<FuncId>, // Start function index
    pub exports: Vec<Export>,
    pub elements: Vec<ElementSegment<'a>>, // Element section
    pub data: Vec<DataSegment<'a>>,
    pub type_names: Vec<String>, // Names for types (debug)
    pub annotations: AnnotationStore<'a>,
}

impl<'a> Module<'a> {
    pub fn new(allocator: &'a bumpalo::Bump) -> Self {
        Self {
            allocator,
            features: FeatureSet::DEFAULT,
            types: Vec::new(),
            functions: IdArena::new(),
            tables: IdArena::new(),
            memories: IdArena::new(),
            globals: IdArena::new(),
            imports: Vec::new(),
            start: None,
            exports: Vec::new(),
            elements: Vec::new(),
            data: Vec::new(),
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

    pub fn add_import(&mut self, import: Import) -> u32 {
        let id = self.imports.len() as u32;

        match &import.kind {
            ImportKind::Function(params, results) => {
                let func_id = self.functions.push(Function {
                    name: import.name.clone(),
                    params: *params,
                    results: *results,
                    vars: Vec::new(),
                    body: None,
                    type_idx: None,
                    local_names: Vec::new(),
                    import: Some(FunctionImport {
                        module: import.module.clone(),
                        name: import.name.clone(),
                    }),
                });
                // We return the func_id cast to u32 for now if needed,
                // but usually we want to return the import index.
            }
            ImportKind::Table(limits) => {
                self.tables.alloc(crate::ir::module::Table {
                    name: import.name.clone(),
                    limits: limits.clone(),
                    kind: crate::ir::module::TableKind::Import {
                        module: import.module.clone(),
                        field: import.name.clone(),
                    },
                });
            }
            ImportKind::Memory(limits) => {
                self.memories.alloc(crate::ir::module::Memory {
                    name: import.name.clone(),
                    limits: limits.clone(),
                    kind: crate::ir::module::MemoryKind::Import {
                        module: import.module.clone(),
                        field: import.name.clone(),
                    },
                });
            }
            ImportKind::Global(type_, mutable) => {
                self.globals.alloc(crate::ir::module::Global {
                    name: import.name.clone(),
                    type_: *type_,
                    mutable: *mutable,
                    kind: crate::ir::module::GlobalKind::Import {
                        module: import.module.clone(),
                        field: import.name.clone(),
                    },
                });
            }
        }

        self.imports.push(import);
        id
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

    pub fn add_data_segment(&mut self, segment: DataSegment<'a>) {
        self.data.push(segment);
    }

    pub fn set_start(&mut self, id: FuncId) {
        self.start = Some(id);
    }

    pub fn add_element_segment(&mut self, segment: ElementSegment<'a>) {
        self.elements.push(segment);
    }

    pub fn add_type(&mut self, params: Type, results: Type) -> u32 {
        let type_idx = self.types.len() as u32;
        self.types.push(FuncType { params, results });
        type_idx
    }

    /// Finds the index of a signature type in the module's type section.
    pub fn find_type_index(&self, ty: Type) -> Option<u32> {
        let sig = crate::ir::type_store::lookup_signature(ty)?;
        self.types
            .iter()
            .position(|t| t.params == sig.params && t.results == sig.results)
            .map(|i| i as u32)
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
