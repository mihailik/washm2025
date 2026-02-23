use crate::collections::id_arena::{Id, IdArena};
use crate::ir::annotation::AnnotationStore;
use crate::ir::expression::{Expression, ExpressionId, ExpressionMarker};
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
pub struct TagMarker;
pub type TagId = Id<TagMarker>;

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub struct LocalMarker;
pub type LocalId = Id<LocalMarker>;

#[derive(Debug, Clone)]
pub struct Local {
    pub name: String,
    pub type_: Type,
    pub h_type: Option<crate::ir::annotation::HighLevelType>,
}

#[derive(Debug, Clone)]
pub struct FunctionImport {
    pub module: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub enum FunctionKind {
    Local {
        locals: IdArena<LocalMarker, Local>,
        param_ids: Vec<LocalId>,
        var_ids: Vec<LocalId>,
        body: ExpressionId,
    },
    Import(FunctionImport),
}

#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub type_idx: Option<TypeId>, // Pointer to module.types
    pub params: Type,
    pub results: Type,
    pub kind: FunctionKind,
}

impl Function {
    pub fn new(
        name: String,
        params: Type,
        results: Type,
        vars: Vec<Type>,
        body: ExpressionId,
    ) -> Self {
        let mut locals: IdArena<LocalMarker, Local> = IdArena::new();
        let mut param_ids = Vec::new();
        for param_type in params.tuple_elements() {
            param_ids.push(
                locals
                    .alloc(Local {
                        name: String::new(),
                        type_: param_type,
                        h_type: None,
                    })
                    .cast(),
            );
        }
        let mut var_ids = Vec::new();
        for var_type in vars {
            var_ids.push(
                locals
                    .alloc(Local {
                        name: String::new(),
                        type_: var_type,
                        h_type: None,
                    })
                    .cast(),
            );
        }
        Self {
            name,
            params,
            results,
            type_idx: None,
            kind: FunctionKind::Local {
                locals,
                param_ids,
                var_ids,
                body,
            },
        }
    }

    pub fn with_type_idx(
        name: String,
        type_idx: TypeId,
        params: Type,
        results: Type,
        vars: Vec<Type>,
        body: ExpressionId,
    ) -> Self {
        let mut locals: IdArena<LocalMarker, Local> = IdArena::new();
        let mut param_ids = Vec::new();
        for param_type in params.tuple_elements() {
            param_ids.push(
                locals
                    .alloc(Local {
                        name: String::new(),
                        type_: param_type,
                        h_type: None,
                    })
                    .cast(),
            );
        }
        let mut var_ids = Vec::new();
        for var_type in vars {
            var_ids.push(
                locals
                    .alloc(Local {
                        name: String::new(),
                        type_: var_type,
                        h_type: None,
                    })
                    .cast(),
            );
        }
        Self {
            name,
            params,
            results,
            type_idx: Some(type_idx),
            kind: FunctionKind::Local {
                locals,
                param_ids,
                var_ids,
                body,
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

    pub fn var_ids(&self) -> &[LocalId] {
        match &self.kind {
            FunctionKind::Local { var_ids, .. } => var_ids,
            FunctionKind::Import(_) => &[],
        }
    }

    pub fn var_ids_mut(&mut self) -> Option<&mut Vec<LocalId>> {
        match &mut self.kind {
            FunctionKind::Local { var_ids, .. } => Some(var_ids),
            FunctionKind::Import(_) => None,
        }
    }

    pub fn body(&self) -> Option<ExpressionId> {
        match &self.kind {
            FunctionKind::Local { body, .. } => Some(*body),
            FunctionKind::Import(_) => None,
        }
    }

    pub fn body_mut(&mut self) -> Option<&mut ExpressionId> {
        match &mut self.kind {
            FunctionKind::Local { body, .. } => Some(body),
            FunctionKind::Import(_) => None,
        }
    }

    pub fn set_body(&mut self, new_body: ExpressionId) {
        if let FunctionKind::Local { body, .. } = &mut self.kind {
            *body = new_body;
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
    pub fn new_for_test(f: Function) -> Self {
        f
    }

    pub fn add_var(&mut self, type_: Type) -> LocalId {
        let id = self
            .locals_mut()
            .alloc(Local {
                name: String::new(),
                type_,
                h_type: None,
            })
            .cast();
        if let Some(var_ids) = self.var_ids_mut() {
            var_ids.push(id);
        }
        id
    }

    pub fn get_local_type(&self, id: LocalId) -> Type {
        self.locals()
            .get(id.cast())
            .map(|l| l.type_)
            .unwrap_or(Type::NONE)
    }

    pub fn vars(&self) -> Vec<Type> {
        self.var_ids()
            .iter()
            .map(|&id| self.get_local_type(id))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemoryLimits {
    pub initial: u32,         // Initial size in 64KB pages
    pub maximum: Option<u32>, // Optional maximum size
}

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
pub struct Global {
    pub name: String,
    pub type_: Type,
    pub mutable: bool,
    pub kind: GlobalKind,
}

#[derive(Debug, Clone)]
pub enum GlobalKind {
    Import { module: String, field: String },
    Local { init: ExpressionId },
}

impl Global {
    pub fn is_import(&self) -> bool {
        matches!(self.kind, GlobalKind::Import { .. })
    }

    pub fn kind_to_init(&self) -> Option<ExpressionId> {
        match self.kind {
            GlobalKind::Local { init } => Some(init),
            GlobalKind::Import { .. } => None,
        }
    }
}

impl Memory {
    pub fn is_import(&self) -> bool {
        matches!(self.kind, MemoryKind::Import { .. })
    }
}

impl Table {
    pub fn is_import(&self) -> bool {
        matches!(self.kind, TableKind::Import { .. })
    }
}

impl GlobalKind {
    pub fn set_init(&mut self, new_init: ExpressionId) {
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

#[derive(Debug, Clone)]
pub struct DataSegment {
    pub memory_index: MemoryId,
    pub offset: ExpressionId, // Offset expression (where in memory)
    pub data: Vec<u8>,        // The actual data bytes
}

#[derive(Debug, Clone)]
pub struct ElementSegment {
    pub table_index: TableId,
    pub offset: ExpressionId,      // Offset expression (where in table)
    pub func_indices: Vec<FuncId>, // Function indices to initialize
}

#[derive(Debug, Clone)]
pub struct Tag {
    pub name: String,
    pub type_idx: TypeId,
}

#[derive(Debug, Clone)]
pub struct CustomSection {
    pub name: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
pub struct Module {
    pub features: FeatureSet,
    pub types: IdArena<TypeMarker, FuncType>, // Type section
    pub functions: IdArena<FuncMarker, Function>,
    pub tables: IdArena<TableMarker, Table>,
    pub memories: IdArena<MemoryMarker, Memory>,
    pub globals: IdArena<GlobalMarker, Global>,
    pub start: Option<FuncId>, // Start function index
    pub exports: Vec<Export>,
    pub elements: IdArena<ElementMarker, ElementSegment>, // Element section
    pub data: IdArena<DataMarker, DataSegment>,
    pub tags: IdArena<TagMarker, Tag>,
    pub custom_sections: Vec<CustomSection>,
    pub type_names: Vec<String>,      // Names for types (debug)
    pub debug_filenames: Vec<String>, // Global source filenames for DWARF
    pub annotations: AnnotationStore,
    pub expressions: IdArena<ExpressionMarker, Expression>,
    // Dummies for 13-tuple matching
    pub imports: Vec<Import>,
    pub strings: Vec<String>,
    pub recgroups: Vec<()>,
}

impl Module {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn builder(&mut self) -> crate::ir::expression::IrBuilder<'_> {
        crate::ir::expression::IrBuilder::with_annotations(
            &mut self.expressions,
            &mut self.annotations,
        )
    }

    pub fn parts_mut(
        &mut self,
    ) -> (
        &mut IdArena<ExpressionMarker, Expression>,
        &mut IdArena<TypeMarker, FuncType>,
        &mut IdArena<FuncMarker, Function>,
        &mut IdArena<GlobalMarker, Global>,
        &mut Vec<Export>,
        &mut Vec<Import>,
        &mut IdArena<TableMarker, Table>,
        &mut IdArena<MemoryMarker, Memory>,
        &mut IdArena<ElementMarker, ElementSegment>,
        &mut IdArena<DataMarker, DataSegment>,
        &mut IdArena<TagMarker, Tag>,
        &mut Vec<String>,
        &mut AnnotationStore,
    ) {
        (
            &mut self.expressions,
            &mut self.types,
            &mut self.functions,
            &mut self.globals,
            &mut self.exports,
            &mut self.imports,
            &mut self.tables,
            &mut self.memories,
            &mut self.elements,
            &mut self.data,
            &mut self.tags,
            &mut self.strings,
            &mut self.annotations,
        )
    }

    pub fn add_function(&mut self, func: Function) -> FuncId {
        self.functions.alloc(func).cast()
    }

    pub fn get_function(&self, id: FuncId) -> Option<&Function> {
        self.functions.get(id.cast())
    }

    pub fn get_function_mut(&mut self, id: FuncId) -> Option<&mut Function> {
        self.functions.get_mut(id.cast())
    }

    pub fn get_function_by_name(&self, name: &str) -> Option<(FuncId, &Function)> {
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

    pub fn add_global(&mut self, global: Global) -> GlobalId {
        self.globals.alloc(global).cast()
    }

    pub fn get_global(&self, id: GlobalId) -> Option<&Global> {
        self.globals.get(id.cast())
    }

    pub fn get_global_by_name(&self, name: &str) -> Option<(GlobalId, &Global)> {
        self.globals
            .iter()
            .find(|(_, g)| g.name == name)
            .map(|(id, g)| (id.cast(), g))
    }

    pub fn export_global(&mut self, id: GlobalId, export_name: String) {
        self.add_export(export_name, ExportKind::Global(id));
    }

    pub fn add_data_segment(&mut self, segment: DataSegment) -> DataId {
        self.data.alloc(segment).cast()
    }

    pub fn set_start(&mut self, id: FuncId) {
        self.start = Some(id);
    }

    pub fn add_element_segment(&mut self, segment: ElementSegment) -> ElementId {
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
        expr: ExpressionId,
        annotation: crate::ir::annotation::Annotation,
    ) {
        self.annotations.insert(expr, annotation);
    }

    pub fn get_annotations(
        &self,
        expr: ExpressionId,
    ) -> Option<crate::ir::annotation::Annotations> {
        self.annotations.get(expr)
    }

    pub fn set_function_local_name(&mut self, _func_idx: usize, _local_idx: u32, _name: &str) {
        // Implementation pending schema decision
    }

    /// Read a WebAssembly module from binary format.
    pub fn read_binary(binary: &[u8]) -> Result<Self, String> {
        let mut reader = crate::wasm::binary_reader::BinaryReader::new();
        reader
            .parse_module(binary)
            .map_err(|e| format!("Binary parsing error: {:?}", e))
    }

    /// Read a WebAssembly module from WAT format using the "binary bridge".
    pub fn read_wat(wat: &str) -> Result<Self, String> {
        let binary = wat::parse_str(wat).map_err(|e| format!("WAT parsing error: {}", e))?;
        let mut reader = crate::wasm::binary_reader::BinaryReader::new();
        reader
            .parse_module(&binary)
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
