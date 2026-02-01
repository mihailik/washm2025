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
pub struct LocalMarker;
pub type LocalId = Id<LocalMarker>;

#[derive(Debug)]
pub struct Function<'a> {
    pub name: String,
    pub type_idx: Option<u32>, // Index into module.types (None means infer from params/results)
    pub params: Type,
    pub results: Type,
    pub vars: Vec<Type>,
    pub body: Option<ExprRef<'a>>,
    pub local_names: Vec<String>,
}

impl<'a> Function<'a> {
    pub fn new(
        name: String,
        params: Type,
        results: Type,
        vars: Vec<Type>,
        body: Option<ExprRef<'a>>,
    ) -> Self {
        let total_locals = params.tuple_len() + vars.len();
        Self {
            name,
            type_idx: None,
            params,
            results,
            vars,
            body,
            local_names: vec![String::new(); total_locals],
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
        let total_locals = params.tuple_len() + vars.len();
        Self {
            name,
            type_idx: Some(type_idx),
            params,
            results,
            vars,
            body,
            local_names: vec![String::new(); total_locals],
        }
    }

    pub fn add_var(&mut self, type_: Type) -> u32 {
        let index = (self.params.tuple_len() + self.vars.len()) as u32;
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
pub struct Export {
    pub name: String,
    pub kind: ExportKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportKind {
    Function(u32),
    Table(u32),
    Memory(u32),
    Global(GlobalId),
}

#[derive(Debug, Clone)]
pub struct Import {
    pub module: String,
    pub name: String,
    pub kind: ImportKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuncType {
    pub params: Type,  // Parameter types
    pub results: Type, // Result types
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImportKind {
    Function(Type, Type),          // params, results
    Table(Type, u32, Option<u32>), // elem_type, min, max
    Memory(MemoryLimits),
}

#[derive(Debug)]
pub struct DataSegment<'a> {
    pub memory_index: u32,   // Memory index (usually 0 in MVP)
    pub offset: ExprRef<'a>, // Offset expression (where in memory)
    pub data: Vec<u8>,       // The actual data bytes
}

#[derive(Debug, Clone, PartialEq)]
pub struct TableLimits {
    pub element_type: Type,   // Element type (funcref in MVP)
    pub initial: u32,         // Initial size
    pub maximum: Option<u32>, // Optional maximum size
}

#[derive(Debug)]
pub struct ElementSegment<'a> {
    pub table_index: u32,       // Table index (usually 0 in MVP)
    pub offset: ExprRef<'a>,    // Offset expression (where in table)
    pub func_indices: Vec<u32>, // Function indices to initialize
}

#[derive(Debug)]
pub struct Module<'a> {
    pub allocator: &'a bumpalo::Bump,
    pub features: FeatureSet,
    pub types: Vec<FuncType>, // Type section
    pub imports: Vec<Import>,
    pub functions: Vec<Function<'a>>,
    pub table: Option<TableLimits>, // Table section (MVP: single table)
    pub globals: IdArena<Global<'a>>,
    pub memory: Option<MemoryLimits>,
    pub start: Option<u32>, // Start function index
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
            imports: Vec::new(),
            functions: Vec::new(),
            table: None,
            globals: IdArena::new(),
            memory: None,
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

    pub fn add_import(&mut self, import: Import) {
        self.imports.push(import);
    }

    pub fn add_function(&mut self, func: Function<'a>) {
        self.functions.push(func);
    }

    pub fn get_function(&self, name: &str) -> Option<&Function<'a>> {
        self.functions.iter().find(|f| f.name == name)
    }

    pub fn get_function_mut(&mut self, name: &str) -> Option<&mut Function<'a>> {
        self.functions.iter_mut().find(|f| f.name == name)
    }

    pub fn set_memory(&mut self, initial: u32, maximum: Option<u32>) {
        self.memory = Some(MemoryLimits { initial, maximum });
    }

    pub fn add_export(&mut self, name: String, kind: ExportKind) {
        self.exports.push(Export { name, kind });
    }

    pub fn export_function(&mut self, func_index: u32, export_name: String) {
        self.add_export(export_name, ExportKind::Function(func_index));
    }

    pub fn add_global(&mut self, global: Global<'a>) -> GlobalId {
        self.globals.alloc(global).cast()
    }

    pub fn get_global(&self, id: GlobalId) -> Option<&Global<'a>> {
        self.globals.get(id.cast())
    }

    pub fn get_global_by_name(&self, name: &str) -> Option<(GlobalId, &Global<'a>)> {
        self.globals.iter().find(|(_, g)| g.name == name).map(|(id, g)| (id.cast(), g))
    }

    pub fn export_global(&mut self, id: GlobalId, export_name: String) {
        self.add_export(export_name, ExportKind::Global(id));
    }

    pub fn add_data_segment(&mut self, segment: DataSegment<'a>) {
        self.data.push(segment);
    }

    pub fn set_start(&mut self, func_index: u32) {
        self.start = Some(func_index);
    }

    pub fn set_table(&mut self, element_type: Type, initial: u32, maximum: Option<u32>) {
        self.table = Some(TableLimits {
            element_type,
            initial,
            maximum,
        });
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
