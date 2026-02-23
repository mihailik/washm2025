use crate::collections::id_arena::{Id, IdArena};
use crate::ir::ops::{BinaryOp, UnaryOp};
use crate::ir::{
    BlockType, DataId, ElementId, FuncId, GlobalId, Literal, LocalId, MemoryId, TableId, Type,
    TypeId,
};

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub struct ExpressionMarker;
pub type ExpressionId = Id<ExpressionMarker>;

#[derive(Debug, Clone)]
pub struct Expression {
    pub type_: Type,
    pub kind: ExpressionKind,
}

#[derive(Debug, Clone)]
pub enum ExpressionKind {
    Block {
        name: Option<String>,
        list: Vec<ExpressionId>,
    },
    Const(Literal),
    Unary {
        op: UnaryOp,
        value: ExpressionId,
    },
    Binary {
        op: BinaryOp,
        left: ExpressionId,
        right: ExpressionId,
    },
    Call {
        target: FuncId,
        operands: Vec<ExpressionId>,
        is_return: bool,
    },
    LocalGet {
        id: LocalId,
    },
    LocalSet {
        id: LocalId,
        value: ExpressionId,
    },
    LocalTee {
        id: LocalId,
        value: ExpressionId,
    },
    GlobalGet {
        id: GlobalId,
    },
    GlobalSet {
        id: GlobalId,
        value: ExpressionId,
    },
    If {
        name: Option<String>,
        condition: ExpressionId,
        if_true: ExpressionId,
        if_false: Option<ExpressionId>,
    },
    Loop {
        name: Option<String>,
        body: ExpressionId,
    },
    Break {
        target: ExpressionId,
        condition: Option<ExpressionId>,
        value: Option<ExpressionId>,
    },
    Return {
        value: Option<ExpressionId>,
    },
    Unreachable,
    Drop {
        value: ExpressionId,
    },
    Select {
        condition: ExpressionId,
        if_true: ExpressionId,
        if_false: ExpressionId,
    },
    Load {
        memory: MemoryId,
        bytes: u32,        // 1, 2, 4, or 8
        signed: bool,      // For sub-word loads
        offset: u32,       // Memory offset
        align: u32,        // Alignment (power of 2)
        ptr: ExpressionId, // Address to load from
    },
    Store {
        memory: MemoryId,
        bytes: u32,          // 1, 2, 4, or 8
        offset: u32,         // Memory offset
        align: u32,          // Alignment
        ptr: ExpressionId,   // Address to store to
        value: ExpressionId, // Value to store
    },
    Nop,
    Switch {
        targets: Vec<ExpressionId>,  // List of target labels
        default: ExpressionId,       // Default label
        condition: ExpressionId,     // Index
        value: Option<ExpressionId>, // Value passing (nullable)
    },
    CallIndirect {
        table: TableId,
        target: ExpressionId, // Function index
        operands: Vec<ExpressionId>,
        type_id: TypeId, // Signature ID
        is_return: bool,
    },
    MemoryGrow {
        memory: MemoryId,
        delta: ExpressionId,
    },
    MemorySize {
        memory: MemoryId,
    },
    // Atomic operations
    AtomicRMW {
        memory: MemoryId,
        op: crate::ir::ops::AtomicOp,
        bytes: u32,
        offset: u32,
        ptr: ExpressionId,
        value: ExpressionId,
    },
    AtomicCmpxchg {
        memory: MemoryId,
        bytes: u32,
        offset: u32,
        ptr: ExpressionId,
        expected: ExpressionId,
        replacement: ExpressionId,
    },
    AtomicWait {
        memory: MemoryId,
        ptr: ExpressionId,
        expected: ExpressionId,
        timeout: ExpressionId,
        expected_type: Type,
    },
    AtomicNotify {
        memory: MemoryId,
        ptr: ExpressionId,
        count: ExpressionId,
    },
    AtomicFence,
    // Tuple operations
    TupleMake {
        operands: Vec<ExpressionId>,
    },
    TupleExtract {
        tuple: ExpressionId,
        index: u32,
    },
    // Reference types
    RefNull {
        type_: Type,
    },
    RefIsNull {
        value: ExpressionId,
    },
    RefFunc {
        func: FuncId,
    },
    RefEq {
        left: ExpressionId,
        right: ExpressionId,
    },
    RefAs {
        op: crate::ir::ops::RefAsOp,
        value: ExpressionId,
    },
    TableGet {
        table: TableId,
        index: ExpressionId,
    },
    TableSet {
        table: TableId,
        index: ExpressionId,
        value: ExpressionId,
    },
    TableSize {
        table: TableId,
    },
    TableGrow {
        table: TableId,
        value: ExpressionId,
        delta: ExpressionId,
    },
    TableFill {
        table: TableId,
        dest: ExpressionId,
        value: ExpressionId,
        size: ExpressionId,
    },
    TableCopy {
        dest_table: TableId,
        src_table: TableId,
        dest: ExpressionId,
        src: ExpressionId,
        size: ExpressionId,
    },
    TableInit {
        table: TableId,
        segment: ElementId,
        dest: ExpressionId,
        offset: ExpressionId,
        size: ExpressionId,
    },
    ElemDrop {
        segment: ElementId,
    },
    MemoryInit {
        memory: MemoryId,
        segment: DataId,
        dest: ExpressionId,
        offset: ExpressionId,
        size: ExpressionId,
    },
    DataDrop {
        segment: DataId,
    },
    MemoryCopy {
        dest_memory: MemoryId,
        src_memory: MemoryId,
        dest: ExpressionId,
        src: ExpressionId,
        size: ExpressionId,
    },
    MemoryFill {
        memory: MemoryId,
        dest: ExpressionId,
        value: ExpressionId,
        size: ExpressionId,
    },
    Pop {
        type_: Type,
    },
    I31New {
        value: ExpressionId,
    },
    I31Get {
        i31: ExpressionId,
        signed: bool,
    },
    SIMDExtract {
        op: crate::ir::ops::SIMDExtractOp,
        vec: ExpressionId,
        index: u8,
    },
    SIMDReplace {
        op: crate::ir::ops::SIMDReplaceOp,
        vec: ExpressionId,
        index: u8,
        value: ExpressionId,
    },
    SIMDShuffle {
        left: ExpressionId,
        right: ExpressionId,
        mask: [u8; 16],
    },
    SIMDTernary {
        op: crate::ir::ops::SIMDTernaryOp,
        a: ExpressionId,
        b: ExpressionId,
        c: ExpressionId,
    },
    SIMDShift {
        op: crate::ir::ops::SIMDShiftOp,
        vec: ExpressionId,
        shift: ExpressionId,
    },
    SIMDLoad {
        op: crate::ir::ops::SIMDLoadOp,
        offset: u32,
        align: u32,
        ptr: ExpressionId,
    },
    SIMDLoadStoreLane {
        op: crate::ir::ops::SIMDLoadStoreLaneOp,
        offset: u32,
        align: u32,
        index: u8,
        is_store: bool,
        ptr: ExpressionId,
        vec: ExpressionId,
    },
    StructNew {
        type_: Type, // Heap type
        operands: Vec<ExpressionId>,
    },
    StructGet {
        type_: Type, // Heap type
        ptr: ExpressionId,
        index: u32,
        signed: bool,
    },
    StructSet {
        type_: Type, // Heap type
        ptr: ExpressionId,
        index: u32,
        value: ExpressionId,
    },
    ArrayNew {
        type_: Type, // Heap type
        size: ExpressionId,
        init: Option<ExpressionId>,
    },
    ArrayGet {
        type_: Type, // Heap type
        ptr: ExpressionId,
        index: ExpressionId,
        signed: bool,
    },
    ArraySet {
        type_: Type, // Heap type
        ptr: ExpressionId,
        index: ExpressionId,
        value: ExpressionId,
    },
    ArrayLen {
        ptr: ExpressionId,
    },
    Try {
        name: Option<String>,
        body: ExpressionId,
        catch_tags: Vec<String>,
        catch_bodies: Vec<ExpressionId>,
        delegate: Option<ExpressionId>,
    },
    Throw {
        tag: String,
        operands: Vec<ExpressionId>,
    },
    Rethrow {
        target: ExpressionId,
    },
    RefTest {
        op: crate::ir::ops::RefTestOp,
        value: ExpressionId,
        type_: Type,
    },
    RefCast {
        op: crate::ir::ops::RefCastOp,
        value: ExpressionId,
        type_: Type,
    },
    BrOn {
        target: ExpressionId,
        op: crate::ir::ops::BrOnOp,
        value: ExpressionId,
        type_: Type,
    },
}

impl Expression {
    pub fn new(kind: ExpressionKind, type_: Type) -> Self {
        Self { kind, type_ }
    }

    pub fn alloc(
        arena: &mut IdArena<ExpressionMarker, Expression>,
        kind: ExpressionKind,
        type_: Type,
    ) -> ExpressionId {
        arena.alloc(Expression { type_, kind })
    }
}

impl ExpressionKind {
    pub fn for_each_child<F>(&self, mut f: F)
    where
        F: FnMut(ExpressionId),
    {
        match self {
            ExpressionKind::Unary { value, .. }
            | ExpressionKind::LocalSet { value, .. }
            | ExpressionKind::LocalTee { value, .. }
            | ExpressionKind::GlobalSet { value, .. }
            | ExpressionKind::Drop { value }
            | ExpressionKind::Load { ptr: value, .. }
            | ExpressionKind::MemoryGrow { delta: value, .. }
            | ExpressionKind::RefIsNull { value }
            | ExpressionKind::RefAs { value, .. }
            | ExpressionKind::RefTest { value, .. }
            | ExpressionKind::RefCast { value, .. }
            | ExpressionKind::BrOn { value, .. }
            | ExpressionKind::I31New { value }
            | ExpressionKind::I31Get { i31: value, .. }
            | ExpressionKind::TupleExtract { tuple: value, .. }
            | ExpressionKind::SIMDExtract { vec: value, .. }
            | ExpressionKind::SIMDLoad { ptr: value, .. }
            | ExpressionKind::StructGet { ptr: value, .. }
            | ExpressionKind::ArrayLen { ptr: value } => {
                f(*value);
            }
            ExpressionKind::Return { value } => {
                if let Some(v) = value {
                    f(*v);
                }
            }
            ExpressionKind::Binary { left, right, .. }
            | ExpressionKind::Store {
                ptr: left,
                value: right,
                ..
            }
            | ExpressionKind::AtomicRMW {
                ptr: left,
                value: right,
                ..
            }
            | ExpressionKind::TableSet {
                index: left,
                value: right,
                ..
            }
            | ExpressionKind::RefEq { left, right }
            | ExpressionKind::AtomicNotify {
                ptr: left,
                count: right,
                ..
            }
            | ExpressionKind::SIMDReplace {
                vec: left,
                value: right,
                ..
            }
            | ExpressionKind::SIMDShuffle { left, right, .. }
            | ExpressionKind::StructSet {
                ptr: left,
                value: right,
                ..
            }
            | ExpressionKind::ArrayGet {
                ptr: left,
                index: right,
                ..
            }
            | ExpressionKind::SIMDShift {
                vec: left,
                shift: right,
                ..
            } => {
                f(*left);
                f(*right);
            }
            ExpressionKind::TableGet { index, .. } => {
                f(*index);
            }
            ExpressionKind::TableGrow { value, delta, .. } => {
                f(*value);
                f(*delta);
            }
            ExpressionKind::Select {
                if_true,
                if_false,
                condition,
            } => {
                f(*if_true);
                f(*if_false);
                f(*condition);
            }
            ExpressionKind::AtomicCmpxchg {
                ptr,
                expected,
                replacement,
                ..
            } => {
                f(*ptr);
                f(*expected);
                f(*replacement);
            }
            ExpressionKind::AtomicWait {
                ptr,
                expected,
                timeout,
                ..
            } => {
                f(*ptr);
                f(*expected);
                f(*timeout);
            }
            ExpressionKind::SIMDTernary { a, b, c, .. } => {
                f(*a);
                f(*b);
                f(*c);
            }
            ExpressionKind::SIMDLoadStoreLane { ptr, vec, .. } => {
                f(*ptr);
                f(*vec);
            }
            ExpressionKind::MemoryInit {
                dest, offset, size, ..
            }
            | ExpressionKind::TableInit {
                dest, offset, size, ..
            } => {
                f(*dest);
                f(*offset);
                f(*size);
            }
            ExpressionKind::MemoryCopy {
                dest, src, size, ..
            }
            | ExpressionKind::TableCopy {
                dest, src, size, ..
            } => {
                f(*dest);
                f(*src);
                f(*size);
            }
            ExpressionKind::MemoryFill {
                dest, value, size, ..
            }
            | ExpressionKind::TableFill {
                dest, value, size, ..
            } => {
                f(*dest);
                f(*value);
                f(*size);
            }
            ExpressionKind::Block { list, .. }
            | ExpressionKind::Call { operands: list, .. }
            | ExpressionKind::TupleMake { operands: list }
            | ExpressionKind::StructNew { operands: list, .. }
            | ExpressionKind::Throw { operands: list, .. } => {
                for id in list {
                    f(*id);
                }
            }
            ExpressionKind::CallIndirect {
                target, operands, ..
            } => {
                f(*target);
                for id in operands {
                    f(*id);
                }
            }
            ExpressionKind::If {
                condition,
                if_true,
                if_false,
                ..
            } => {
                f(*condition);
                f(*if_true);
                if let Some(f_id) = if_false {
                    f(*f_id);
                }
            }
            ExpressionKind::Loop { body, .. } => {
                f(*body);
            }
            ExpressionKind::Break {
                condition, value, ..
            } => {
                if let Some(c) = condition {
                    f(*c);
                }
                if let Some(v) = value {
                    f(*v);
                }
            }
            ExpressionKind::Switch {
                condition, value, ..
            } => {
                f(*condition);
                if let Some(v) = value {
                    f(*v);
                }
            }
            ExpressionKind::ArrayNew { size, init, .. } => {
                f(*size);
                if let Some(i) = init {
                    f(*i);
                }
            }
            ExpressionKind::ArraySet {
                ptr, index, value, ..
            } => {
                f(*ptr);
                f(*index);
                f(*value);
            }
            ExpressionKind::Try {
                body, catch_bodies, ..
            } => {
                f(*body);
                for &b in catch_bodies {
                    f(b);
                }
            }
            ExpressionKind::Const(_)
            | ExpressionKind::LocalGet { .. }
            | ExpressionKind::GlobalGet { .. }
            | ExpressionKind::Unreachable
            | ExpressionKind::Nop
            | ExpressionKind::AtomicFence
            | ExpressionKind::RefNull { .. }
            | ExpressionKind::RefFunc { .. }
            | ExpressionKind::ElemDrop { .. }
            | ExpressionKind::DataDrop { .. }
            | ExpressionKind::MemorySize { .. }
            | ExpressionKind::TableSize { .. }
            | ExpressionKind::Pop { .. }
            | ExpressionKind::Rethrow { .. } => {}
        }
    }

    pub fn for_each_child_mut<F>(&mut self, mut f: F)
    where
        F: FnMut(&mut ExpressionId),
    {
        match self {
            ExpressionKind::Unary { value, .. }
            | ExpressionKind::LocalSet { value, .. }
            | ExpressionKind::LocalTee { value, .. }
            | ExpressionKind::GlobalSet { value, .. }
            | ExpressionKind::Drop { value }
            | ExpressionKind::Load { ptr: value, .. }
            | ExpressionKind::MemoryGrow { delta: value, .. }
            | ExpressionKind::RefIsNull { value }
            | ExpressionKind::RefAs { value, .. }
            | ExpressionKind::RefTest { value, .. }
            | ExpressionKind::RefCast { value, .. }
            | ExpressionKind::BrOn { value, .. }
            | ExpressionKind::I31New { value }
            | ExpressionKind::I31Get { i31: value, .. }
            | ExpressionKind::TupleExtract { tuple: value, .. }
            | ExpressionKind::SIMDExtract { vec: value, .. }
            | ExpressionKind::SIMDLoad { ptr: value, .. }
            | ExpressionKind::StructGet { ptr: value, .. }
            | ExpressionKind::ArrayLen { ptr: value } => {
                f(value);
            }
            ExpressionKind::Return { value: Some(v) } => {
                f(v);
            }
            ExpressionKind::Return { value: None } => {}
            ExpressionKind::Binary { left, right, .. }
            | ExpressionKind::Store {
                ptr: left,
                value: right,
                ..
            }
            | ExpressionKind::AtomicRMW {
                ptr: left,
                value: right,
                ..
            }
            | ExpressionKind::TableSet {
                index: left,
                value: right,
                ..
            }
            | ExpressionKind::RefEq { left, right }
            | ExpressionKind::AtomicNotify {
                ptr: left,
                count: right,
                ..
            }
            | ExpressionKind::SIMDReplace {
                vec: left,
                value: right,
                ..
            }
            | ExpressionKind::SIMDShuffle { left, right, .. }
            | ExpressionKind::StructSet {
                ptr: left,
                value: right,
                ..
            }
            | ExpressionKind::ArrayGet {
                ptr: left,
                index: right,
                ..
            }
            | ExpressionKind::SIMDShift {
                vec: left,
                shift: right,
                ..
            } => {
                f(left);
                f(right);
            }
            ExpressionKind::TableGet { index, .. } => {
                f(index);
            }
            ExpressionKind::TableGrow { value, delta, .. } => {
                f(value);
                f(delta);
            }
            ExpressionKind::Select {
                if_true,
                if_false,
                condition,
            } => {
                f(if_true);
                f(if_false);
                f(condition);
            }
            ExpressionKind::AtomicCmpxchg {
                ptr,
                expected,
                replacement,
                ..
            } => {
                f(ptr);
                f(expected);
                f(replacement);
            }
            ExpressionKind::AtomicWait {
                ptr,
                expected,
                timeout,
                ..
            } => {
                f(ptr);
                f(expected);
                f(timeout);
            }
            ExpressionKind::SIMDTernary { a, b, c, .. } => {
                f(a);
                f(b);
                f(c);
            }
            ExpressionKind::SIMDLoadStoreLane { ptr, vec, .. } => {
                f(ptr);
                f(vec);
            }
            ExpressionKind::MemoryInit {
                dest, offset, size, ..
            }
            | ExpressionKind::TableInit {
                dest, offset, size, ..
            } => {
                f(dest);
                f(offset);
                f(size);
            }
            ExpressionKind::MemoryCopy {
                dest, src, size, ..
            }
            | ExpressionKind::TableCopy {
                dest, src, size, ..
            } => {
                f(dest);
                f(src);
                f(size);
            }
            ExpressionKind::MemoryFill {
                dest, value, size, ..
            }
            | ExpressionKind::TableFill {
                dest, value, size, ..
            } => {
                f(dest);
                f(value);
                f(size);
            }
            ExpressionKind::Block { list, .. }
            | ExpressionKind::Call { operands: list, .. }
            | ExpressionKind::TupleMake { operands: list }
            | ExpressionKind::StructNew { operands: list, .. }
            | ExpressionKind::Throw { operands: list, .. } => {
                for id in list {
                    f(id);
                }
            }
            ExpressionKind::CallIndirect {
                target, operands, ..
            } => {
                f(target);
                for id in operands {
                    f(id);
                }
            }
            ExpressionKind::If {
                condition,
                if_true,
                if_false,
                ..
            } => {
                f(condition);
                f(if_true);
                if let Some(v) = if_false {
                    f(v);
                }
            }
            ExpressionKind::Loop { body, .. } => {
                f(body);
            }
            ExpressionKind::Break {
                condition, value, ..
            } => {
                if let Some(v) = condition {
                    f(v);
                }
                if let Some(v) = value {
                    f(v);
                }
            }
            ExpressionKind::Switch {
                condition, value, ..
            } => {
                f(condition);
                if let Some(v) = value {
                    f(v);
                }
            }
            ExpressionKind::ArrayNew { size, init, .. } => {
                f(size);
                if let Some(i) = init {
                    f(i);
                }
            }
            ExpressionKind::ArraySet {
                ptr, index, value, ..
            } => {
                f(ptr);
                f(index);
                f(value);
            }
            ExpressionKind::Try {
                body, catch_bodies, ..
            } => {
                f(body);
                for b in catch_bodies {
                    f(b);
                }
            }
            _ => {}
        }
    }
}

// Helpers for construction

pub trait LabelName {
    fn to_option_string(self) -> Option<String>;
}

impl LabelName for String {
    fn to_option_string(self) -> Option<String> {
        if self.is_empty() {
            None
        } else {
            Some(self)
        }
    }
}

impl LabelName for &str {
    fn to_option_string(self) -> Option<String> {
        if self.is_empty() {
            None
        } else {
            Some(self.to_string())
        }
    }
}

impl LabelName for Option<String> {
    fn to_option_string(self) -> Option<String> {
        self.and_then(|s| if s.is_empty() { None } else { Some(s) })
    }
}

impl LabelName for Option<&str> {
    fn to_option_string(self) -> Option<String> {
        self.and_then(|s| {
            if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        })
    }
}

pub struct IrBuilder<'a> {
    pub arena: &'a mut IdArena<ExpressionMarker, Expression>,
    pub annotations: Option<&'a mut crate::ir::annotation::AnnotationStore>,
    pub current_location: Option<crate::ir::annotation::DebugLocation>,
}

impl<'a> IrBuilder<'a> {
    pub fn new(arena: &'a mut IdArena<ExpressionMarker, Expression>) -> Self {
        Self {
            arena,
            annotations: None,
            current_location: None,
        }
    }

    pub fn with_annotations(
        arena: &'a mut IdArena<ExpressionMarker, Expression>,
        annotations: &'a mut crate::ir::annotation::AnnotationStore,
    ) -> Self {
        Self {
            arena,
            annotations: Some(annotations),
            current_location: None,
        }
    }

    pub fn set_location(&mut self, location: Option<crate::ir::annotation::DebugLocation>) {
        self.current_location = location;
    }

    pub fn copy_location(&mut self, other: ExpressionId) {
        if let Some(annotations) = &self.annotations {
            self.current_location = annotations.get_location(other);
        }
    }

    fn alloc(&mut self, kind: ExpressionKind, type_: Type) -> ExpressionId {
        let id = self.arena.alloc(Expression { type_, kind });
        if let Some(location) = self.current_location {
            if let Some(annotations) = &mut self.annotations {
                annotations.set_location(id, location);
            }
        }
        id
    }

    pub fn nop(&mut self) -> ExpressionId {
        self.alloc(ExpressionKind::Nop, Type::NONE)
    }

    pub fn const_(&mut self, value: Literal) -> ExpressionId {
        let type_ = value.get_type();
        self.alloc(ExpressionKind::Const(value), type_)
    }

    pub fn block(
        &mut self,
        name: impl LabelName,
        list: Vec<ExpressionId>,
        type_: impl Into<BlockType>,
    ) -> ExpressionId {
        let name = name.to_option_string();
        let type_ = type_.into();
        let actual_type = match type_ {
            BlockType::Empty => Type::NONE,
            BlockType::Type(t) => t,
        };
        self.alloc(ExpressionKind::Block { name, list }, actual_type)
    }

    pub fn unary(&mut self, op: UnaryOp, value: ExpressionId, type_: Type) -> ExpressionId {
        self.alloc(ExpressionKind::Unary { op, value }, type_)
    }

    pub fn binary(
        &mut self,
        op: BinaryOp,
        left: ExpressionId,
        right: ExpressionId,
        type_: Type,
    ) -> ExpressionId {
        self.alloc(ExpressionKind::Binary { op, left, right }, type_)
    }

    pub fn drop(&mut self, value: ExpressionId) -> ExpressionId {
        self.alloc(ExpressionKind::Drop { value }, Type::NONE)
    }

    pub fn call(
        &mut self,
        target: impl Into<FuncId>,
        operands: Vec<ExpressionId>,
        type_: Type,
        is_return: bool,
    ) -> ExpressionId {
        self.alloc(
            ExpressionKind::Call {
                target: target.into(),
                operands,
                is_return,
            },
            type_,
        )
    }

    pub fn call_indirect(
        &mut self,
        table: impl Into<TableId>,
        target: ExpressionId,
        operands: Vec<ExpressionId>,
        type_id: TypeId,
        type_: Type,
        is_return: bool,
    ) -> ExpressionId {
        self.alloc(
            ExpressionKind::CallIndirect {
                table: table.into(),
                target,
                operands,
                type_id,
                is_return,
            },
            type_,
        )
    }

    pub fn local_get(&mut self, id: impl Into<LocalId>, type_: Type) -> ExpressionId {
        self.alloc(ExpressionKind::LocalGet { id: id.into() }, type_)
    }

    pub fn local_set(&mut self, id: impl Into<LocalId>, value: ExpressionId) -> ExpressionId {
        self.alloc(
            ExpressionKind::LocalSet {
                id: id.into(),
                value,
            },
            Type::NONE,
        )
    }

    pub fn local_tee(
        &mut self,
        id: impl Into<LocalId>,
        value: ExpressionId,
        type_: Type,
    ) -> ExpressionId {
        self.alloc(
            ExpressionKind::LocalTee {
                id: id.into(),
                value,
            },
            type_,
        )
    }

    pub fn global_get(&mut self, id: impl Into<GlobalId>, type_: Type) -> ExpressionId {
        self.alloc(ExpressionKind::GlobalGet { id: id.into() }, type_)
    }

    pub fn global_set(&mut self, id: impl Into<GlobalId>, value: ExpressionId) -> ExpressionId {
        self.alloc(
            ExpressionKind::GlobalSet {
                id: id.into(),
                value,
            },
            Type::NONE,
        )
    }

    pub fn switch(
        &mut self,
        targets: Vec<ExpressionId>,
        default: ExpressionId,
        condition: ExpressionId,
        value: Option<ExpressionId>,
    ) -> ExpressionId {
        self.alloc(
            ExpressionKind::Switch {
                targets,
                default,
                condition,
                value,
            },
            Type::UNREACHABLE,
        )
    }

    pub fn if_(
        &mut self,
        name: impl LabelName,
        condition: ExpressionId,
        if_true: ExpressionId,
        if_false: Option<ExpressionId>,
        type_: impl Into<BlockType>,
    ) -> ExpressionId {
        let name = name.to_option_string();
        let type_ = type_.into();
        let actual_type = match type_ {
            BlockType::Empty => Type::NONE,
            BlockType::Type(t) => t,
        };
        self.alloc(
            ExpressionKind::If {
                name,
                condition,
                if_true,
                if_false,
            },
            actual_type,
        )
    }

    pub fn loop_(
        &mut self,
        name: impl LabelName,
        body: ExpressionId,
        type_: impl Into<BlockType>,
    ) -> ExpressionId {
        let name = name.to_option_string();
        let type_ = type_.into();
        let actual_type = match type_ {
            BlockType::Empty => Type::NONE,
            BlockType::Type(t) => t,
        };
        self.alloc(ExpressionKind::Loop { name, body }, actual_type)
    }

    pub fn break_(
        &mut self,
        target: ExpressionId,
        condition: Option<ExpressionId>,
        value: Option<ExpressionId>,
        type_: Type,
    ) -> ExpressionId {
        self.alloc(
            ExpressionKind::Break {
                target,
                condition,
                value,
            },
            type_,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn load(
        &mut self,
        memory: impl Into<MemoryId>,
        bytes: u32,
        signed: bool,
        offset: u32,
        align: u32,
        ptr: ExpressionId,
        type_: Type,
    ) -> ExpressionId {
        self.alloc(
            ExpressionKind::Load {
                memory: memory.into(),
                bytes,
                signed,
                offset,
                align,
                ptr,
            },
            type_,
        )
    }

    pub fn store(
        &mut self,
        memory: impl Into<MemoryId>,
        bytes: u32,
        offset: u32,
        align: u32,
        ptr: ExpressionId,
        value: ExpressionId,
    ) -> ExpressionId {
        self.alloc(
            ExpressionKind::Store {
                memory: memory.into(),
                bytes,
                offset,
                align,
                ptr,
                value,
            },
            Type::NONE,
        )
    }

    pub fn return_(&mut self, value: Option<ExpressionId>) -> ExpressionId {
        self.alloc(ExpressionKind::Return { value }, Type::UNREACHABLE)
    }

    pub fn unreachable(&mut self) -> ExpressionId {
        self.alloc(ExpressionKind::Unreachable, Type::UNREACHABLE)
    }

    pub fn memory_size(&mut self, memory: impl Into<MemoryId>) -> ExpressionId {
        self.alloc(
            ExpressionKind::MemorySize {
                memory: memory.into(),
            },
            Type::I32,
        )
    }

    pub fn select(
        &mut self,
        condition: ExpressionId,
        if_true: ExpressionId,
        if_false: ExpressionId,
        type_: Type,
    ) -> ExpressionId {
        self.alloc(
            ExpressionKind::Select {
                condition,
                if_true,
                if_false,
            },
            type_,
        )
    }

    pub fn memory_grow(
        &mut self,
        memory: impl Into<MemoryId>,
        delta: ExpressionId,
    ) -> ExpressionId {
        self.alloc(
            ExpressionKind::MemoryGrow {
                memory: memory.into(),
                delta,
            },
            Type::I32,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn atomic_rmw(
        &mut self,
        memory: MemoryId,
        op: crate::ir::ops::AtomicOp,
        bytes: u32,
        offset: u32,
        ptr: ExpressionId,
        value: ExpressionId,
        type_: Type,
    ) -> ExpressionId {
        self.alloc(
            ExpressionKind::AtomicRMW {
                memory,
                op,
                bytes,
                offset,
                ptr,
                value,
            },
            type_,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn atomic_cmpxchg(
        &mut self,
        memory: MemoryId,
        bytes: u32,
        offset: u32,
        ptr: ExpressionId,
        expected: ExpressionId,
        replacement: ExpressionId,
        type_: Type,
    ) -> ExpressionId {
        self.alloc(
            ExpressionKind::AtomicCmpxchg {
                memory,
                bytes,
                offset,
                ptr,
                expected,
                replacement,
            },
            type_,
        )
    }

    pub fn atomic_wait(
        &mut self,
        memory: MemoryId,
        ptr: ExpressionId,
        expected: ExpressionId,
        timeout: ExpressionId,
        expected_type: Type,
    ) -> ExpressionId {
        self.alloc(
            ExpressionKind::AtomicWait {
                memory,
                ptr,
                expected,
                timeout,
                expected_type,
            },
            Type::I32,
        )
    }

    pub fn atomic_notify(
        &mut self,
        memory: MemoryId,
        ptr: ExpressionId,
        count: ExpressionId,
    ) -> ExpressionId {
        self.alloc(
            ExpressionKind::AtomicNotify { memory, ptr, count },
            Type::I32,
        )
    }

    pub fn atomic_fence(&mut self) -> ExpressionId {
        self.alloc(ExpressionKind::AtomicFence, Type::NONE)
    }

    pub fn memory_init(
        &mut self,
        memory: MemoryId,
        segment: DataId,
        dest: ExpressionId,
        offset: ExpressionId,
        size: ExpressionId,
    ) -> ExpressionId {
        self.alloc(
            ExpressionKind::MemoryInit {
                memory,
                segment,
                dest,
                offset,
                size,
            },
            Type::NONE,
        )
    }

    pub fn data_drop(&mut self, segment: DataId) -> ExpressionId {
        self.alloc(ExpressionKind::DataDrop { segment }, Type::NONE)
    }

    pub fn memory_copy(
        &mut self,
        dest_memory: MemoryId,
        src_memory: MemoryId,
        dest: ExpressionId,
        src: ExpressionId,
        size: ExpressionId,
    ) -> ExpressionId {
        self.alloc(
            ExpressionKind::MemoryCopy {
                dest_memory,
                src_memory,
                dest,
                src,
                size,
            },
            Type::NONE,
        )
    }

    pub fn memory_fill(
        &mut self,
        memory: MemoryId,
        dest: ExpressionId,
        value: ExpressionId,
        size: ExpressionId,
    ) -> ExpressionId {
        self.alloc(
            ExpressionKind::MemoryFill {
                memory,
                dest,
                value,
                size,
            },
            Type::NONE,
        )
    }

    pub fn table_get(&mut self, table: TableId, index: ExpressionId, type_: Type) -> ExpressionId {
        self.alloc(ExpressionKind::TableGet { table, index }, type_)
    }

    pub fn table_set(
        &mut self,
        table: TableId,
        index: ExpressionId,
        value: ExpressionId,
    ) -> ExpressionId {
        self.alloc(
            ExpressionKind::TableSet {
                table,
                index,
                value,
            },
            Type::NONE,
        )
    }

    pub fn table_size(&mut self, table: TableId) -> ExpressionId {
        self.alloc(ExpressionKind::TableSize { table }, Type::I32)
    }

    pub fn table_grow(
        &mut self,
        table: TableId,
        delta: ExpressionId,
        value: ExpressionId,
    ) -> ExpressionId {
        self.alloc(
            ExpressionKind::TableGrow {
                table,
                delta,
                value,
            },
            Type::I32,
        )
    }

    pub fn table_fill(
        &mut self,
        table: TableId,
        dest: ExpressionId,
        value: ExpressionId,
        size: ExpressionId,
    ) -> ExpressionId {
        self.alloc(
            ExpressionKind::TableFill {
                table,
                dest,
                value,
                size,
            },
            Type::NONE,
        )
    }

    pub fn table_copy(
        &mut self,
        dest_table: TableId,
        src_table: TableId,
        dest: ExpressionId,
        src: ExpressionId,
        size: ExpressionId,
    ) -> ExpressionId {
        self.alloc(
            ExpressionKind::TableCopy {
                dest_table,
                src_table,
                dest,
                src,
                size,
            },
            Type::NONE,
        )
    }

    pub fn table_init(
        &mut self,
        table: TableId,
        segment: ElementId,
        dest: ExpressionId,
        offset: ExpressionId,
        size: ExpressionId,
    ) -> ExpressionId {
        self.alloc(
            ExpressionKind::TableInit {
                table,
                segment,
                dest,
                offset,
                size,
            },
            Type::NONE,
        )
    }

    pub fn elem_drop(&mut self, segment: ElementId) -> ExpressionId {
        self.alloc(ExpressionKind::ElemDrop { segment }, Type::NONE)
    }

    pub fn simd_extract(
        &mut self,
        op: crate::ir::ops::SIMDExtractOp,
        vec: ExpressionId,
        index: u8,
        type_: Type,
    ) -> ExpressionId {
        self.alloc(ExpressionKind::SIMDExtract { op, vec, index }, type_)
    }

    pub fn simd_replace(
        &mut self,
        op: crate::ir::ops::SIMDReplaceOp,
        vec: ExpressionId,
        index: u8,
        value: ExpressionId,
    ) -> ExpressionId {
        self.alloc(
            ExpressionKind::SIMDReplace {
                op,
                vec,
                index,
                value,
            },
            Type::V128,
        )
    }

    pub fn simd_shuffle(
        &mut self,
        left: ExpressionId,
        right: ExpressionId,
        mask: [u8; 16],
    ) -> ExpressionId {
        self.alloc(
            ExpressionKind::SIMDShuffle { left, right, mask },
            Type::V128,
        )
    }

    pub fn simd_ternary(
        &mut self,
        op: crate::ir::ops::SIMDTernaryOp,
        a: ExpressionId,
        b: ExpressionId,
        c: ExpressionId,
    ) -> ExpressionId {
        self.alloc(ExpressionKind::SIMDTernary { op, a, b, c }, Type::V128)
    }

    pub fn simd_shift(
        &mut self,
        op: crate::ir::ops::SIMDShiftOp,
        vec: ExpressionId,
        shift: ExpressionId,
    ) -> ExpressionId {
        self.alloc(ExpressionKind::SIMDShift { op, vec, shift }, Type::V128)
    }

    pub fn simd_load(
        &mut self,
        op: crate::ir::ops::SIMDLoadOp,
        offset: u32,
        align: u32,
        ptr: ExpressionId,
    ) -> ExpressionId {
        self.alloc(
            ExpressionKind::SIMDLoad {
                op,
                offset,
                align,
                ptr,
            },
            Type::V128,
        )
    }

    pub fn ref_null(&mut self, type_: Type) -> ExpressionId {
        self.alloc(ExpressionKind::RefNull { type_ }, type_)
    }

    pub fn ref_is_null(&mut self, value: ExpressionId) -> ExpressionId {
        self.alloc(ExpressionKind::RefIsNull { value }, Type::I32)
    }

    pub fn ref_func(&mut self, func: FuncId) -> ExpressionId {
        self.alloc(ExpressionKind::RefFunc { func }, Type::FUNCREF)
    }

    pub fn ref_eq(&mut self, left: ExpressionId, right: ExpressionId) -> ExpressionId {
        self.alloc(ExpressionKind::RefEq { left, right }, Type::I32)
    }

    pub fn i31_new(&mut self, value: ExpressionId) -> ExpressionId {
        self.alloc(ExpressionKind::I31New { value }, Type::I31REF)
    }

    pub fn i31_get(&mut self, i31: ExpressionId, signed: bool) -> ExpressionId {
        self.alloc(ExpressionKind::I31Get { i31, signed }, Type::I32)
    }

    pub fn struct_new(&mut self, type_: Type, operands: Vec<ExpressionId>) -> ExpressionId {
        self.alloc(ExpressionKind::StructNew { type_, operands }, type_)
    }

    pub fn struct_get(
        &mut self,
        type_: Type,
        ptr: ExpressionId,
        index: u32,
        signed: bool,
        field_type: Type,
    ) -> ExpressionId {
        self.alloc(
            ExpressionKind::StructGet {
                type_,
                ptr,
                index,
                signed,
            },
            field_type,
        )
    }

    pub fn struct_set(
        &mut self,
        type_: Type,
        ptr: ExpressionId,
        index: u32,
        value: ExpressionId,
    ) -> ExpressionId {
        self.alloc(
            ExpressionKind::StructSet {
                type_,
                ptr,
                index,
                value,
            },
            Type::NONE,
        )
    }

    pub fn array_new(
        &mut self,
        type_: Type,
        size: ExpressionId,
        init: Option<ExpressionId>,
    ) -> ExpressionId {
        self.alloc(ExpressionKind::ArrayNew { type_, size, init }, type_)
    }

    pub fn array_get(
        &mut self,
        type_: Type,
        ptr: ExpressionId,
        index: ExpressionId,
        signed: bool,
        field_type: Type,
    ) -> ExpressionId {
        Expression::alloc(
            self.arena,
            ExpressionKind::ArrayGet {
                type_,
                ptr,
                index,
                signed,
            },
            field_type,
        )
    }

    pub fn array_set(
        &mut self,
        type_: Type,
        ptr: ExpressionId,
        index: ExpressionId,
        value: ExpressionId,
    ) -> ExpressionId {
        Expression::alloc(
            self.arena,
            ExpressionKind::ArraySet {
                type_,
                ptr,
                index,
                value,
            },
            Type::NONE,
        )
    }

    pub fn array_len(&mut self, ptr: ExpressionId) -> ExpressionId {
        Expression::alloc(self.arena, ExpressionKind::ArrayLen { ptr }, Type::I32)
    }

    pub fn ref_test(
        &mut self,
        op: crate::ir::ops::RefTestOp,
        value: ExpressionId,
        type_: Type,
    ) -> ExpressionId {
        Expression::alloc(
            self.arena,
            ExpressionKind::RefTest { op, value, type_ },
            Type::I32,
        )
    }

    pub fn ref_cast(
        &mut self,
        op: crate::ir::ops::RefCastOp,
        value: ExpressionId,
        type_: Type,
    ) -> ExpressionId {
        Expression::alloc(
            self.arena,
            ExpressionKind::RefCast { op, value, type_ },
            type_,
        )
    }

    pub fn br_on(
        &mut self,
        target: ExpressionId,
        op: crate::ir::ops::BrOnOp,
        value: ExpressionId,
        type_: Type,
    ) -> ExpressionId {
        Expression::alloc(
            self.arena,
            ExpressionKind::BrOn {
                target,
                op,
                value,
                type_,
            },
            type_,
        )
    }

    /// Deep clone an expression tree
    pub fn deep_clone(&mut self, id: ExpressionId) -> ExpressionId {
        let (kind, type_) = {
            let expr = &self.arena[id];
            (expr.kind.clone(), expr.type_)
        };
        let cloned_kind = kind.clone_in_arena(self);
        Expression::alloc(self.arena, cloned_kind, type_)
    }
}

impl ExpressionKind {
    pub fn clone_in_arena(&self, builder: &mut IrBuilder) -> ExpressionKind {
        match self {
            ExpressionKind::Block { name, list } => ExpressionKind::Block {
                name: name.clone(),
                list: list.iter().map(|&c| builder.deep_clone(c)).collect(),
            },
            ExpressionKind::Const(lit) => ExpressionKind::Const(*lit),
            ExpressionKind::Unary { op, value } => ExpressionKind::Unary {
                op: *op,
                value: builder.deep_clone(*value),
            },
            ExpressionKind::Binary { op, left, right } => ExpressionKind::Binary {
                op: *op,
                left: builder.deep_clone(*left),
                right: builder.deep_clone(*right),
            },
            ExpressionKind::Call {
                target,
                operands,
                is_return,
            } => ExpressionKind::Call {
                target: *target,
                operands: operands.iter().map(|&c| builder.deep_clone(c)).collect(),
                is_return: *is_return,
            },
            ExpressionKind::LocalGet { id } => ExpressionKind::LocalGet { id: *id },
            ExpressionKind::LocalSet { id, value } => ExpressionKind::LocalSet {
                id: *id,
                value: builder.deep_clone(*value),
            },
            ExpressionKind::LocalTee { id, value } => ExpressionKind::LocalTee {
                id: *id,
                value: builder.deep_clone(*value),
            },
            ExpressionKind::GlobalGet { id } => ExpressionKind::GlobalGet { id: *id },
            ExpressionKind::GlobalSet { id, value } => ExpressionKind::GlobalSet {
                id: *id,
                value: builder.deep_clone(*value),
            },
            ExpressionKind::If {
                name,
                condition,
                if_true,
                if_false,
            } => ExpressionKind::If {
                name: name.clone(),
                condition: builder.deep_clone(*condition),
                if_true: builder.deep_clone(*if_true),
                if_false: if_false.map(|e| builder.deep_clone(e)),
            },
            ExpressionKind::Loop { name, body } => ExpressionKind::Loop {
                name: name.clone(),
                body: builder.deep_clone(*body),
            },
            ExpressionKind::Break {
                target,
                condition,
                value,
            } => ExpressionKind::Break {
                target: *target,
                condition: condition.map(|e| builder.deep_clone(e)),
                value: value.map(|e| builder.deep_clone(e)),
            },
            ExpressionKind::Return { value } => ExpressionKind::Return {
                value: value.map(|e| builder.deep_clone(e)),
            },
            ExpressionKind::Unreachable => ExpressionKind::Unreachable,
            ExpressionKind::Drop { value } => ExpressionKind::Drop {
                value: builder.deep_clone(*value),
            },
            ExpressionKind::Select {
                condition,
                if_true,
                if_false,
            } => ExpressionKind::Select {
                condition: builder.deep_clone(*condition),
                if_true: builder.deep_clone(*if_true),
                if_false: builder.deep_clone(*if_false),
            },
            ExpressionKind::Load {
                memory,
                bytes,
                signed,
                offset,
                align,
                ptr,
            } => ExpressionKind::Load {
                memory: *memory,
                bytes: *bytes,
                signed: *signed,
                offset: *offset,
                align: *align,
                ptr: builder.deep_clone(*ptr),
            },
            ExpressionKind::Store {
                memory,
                bytes,
                offset,
                align,
                ptr,
                value,
            } => ExpressionKind::Store {
                memory: *memory,
                bytes: *bytes,
                offset: *offset,
                align: *align,
                ptr: builder.deep_clone(*ptr),
                value: builder.deep_clone(*value),
            },
            ExpressionKind::Nop => ExpressionKind::Nop,
            ExpressionKind::Switch {
                targets,
                default,
                condition,
                value,
            } => ExpressionKind::Switch {
                targets: targets.clone(),
                default: *default,
                condition: builder.deep_clone(*condition),
                value: value.map(|e| builder.deep_clone(e)),
            },
            ExpressionKind::CallIndirect {
                table,
                target,
                operands,
                type_id,
                is_return,
            } => ExpressionKind::CallIndirect {
                table: *table,
                target: builder.deep_clone(*target),
                operands: operands.iter().map(|&c| builder.deep_clone(c)).collect(),
                type_id: *type_id,
                is_return: *is_return,
            },
            ExpressionKind::MemoryGrow { memory, delta } => ExpressionKind::MemoryGrow {
                memory: *memory,
                delta: builder.deep_clone(*delta),
            },
            ExpressionKind::MemorySize { memory } => ExpressionKind::MemorySize { memory: *memory },
            ExpressionKind::AtomicRMW {
                memory,
                op,
                bytes,
                offset,
                ptr,
                value,
            } => ExpressionKind::AtomicRMW {
                memory: *memory,
                op: *op,
                bytes: *bytes,
                offset: *offset,
                ptr: builder.deep_clone(*ptr),
                value: builder.deep_clone(*value),
            },
            ExpressionKind::AtomicCmpxchg {
                memory,
                bytes,
                offset,
                ptr,
                expected,
                replacement,
            } => ExpressionKind::AtomicCmpxchg {
                memory: *memory,
                bytes: *bytes,
                offset: *offset,
                ptr: builder.deep_clone(*ptr),
                expected: builder.deep_clone(*expected),
                replacement: builder.deep_clone(*replacement),
            },
            ExpressionKind::AtomicWait {
                memory,
                ptr,
                expected,
                timeout,
                expected_type,
            } => ExpressionKind::AtomicWait {
                memory: *memory,
                ptr: builder.deep_clone(*ptr),
                expected: builder.deep_clone(*expected),
                timeout: builder.deep_clone(*timeout),
                expected_type: *expected_type,
            },
            ExpressionKind::AtomicNotify { memory, ptr, count } => ExpressionKind::AtomicNotify {
                memory: *memory,
                ptr: builder.deep_clone(*ptr),
                count: builder.deep_clone(*count),
            },
            ExpressionKind::AtomicFence => ExpressionKind::AtomicFence,
            ExpressionKind::SIMDExtract { op, vec, index } => ExpressionKind::SIMDExtract {
                op: *op,
                vec: builder.deep_clone(*vec),
                index: *index,
            },
            ExpressionKind::SIMDReplace {
                op,
                vec,
                index,
                value,
            } => ExpressionKind::SIMDReplace {
                op: *op,
                vec: builder.deep_clone(*vec),
                index: *index,
                value: builder.deep_clone(*value),
            },
            ExpressionKind::SIMDShuffle { left, right, mask } => ExpressionKind::SIMDShuffle {
                left: builder.deep_clone(*left),
                right: builder.deep_clone(*right),
                mask: *mask,
            },
            ExpressionKind::SIMDTernary { op, a, b, c } => ExpressionKind::SIMDTernary {
                op: *op,
                a: builder.deep_clone(*a),
                b: builder.deep_clone(*b),
                c: builder.deep_clone(*c),
            },
            ExpressionKind::SIMDShift { op, vec, shift } => ExpressionKind::SIMDShift {
                op: *op,
                vec: builder.deep_clone(*vec),
                shift: builder.deep_clone(*shift),
            },
            ExpressionKind::SIMDLoad {
                op,
                offset,
                align,
                ptr,
            } => ExpressionKind::SIMDLoad {
                op: *op,
                offset: *offset,
                align: *align,
                ptr: builder.deep_clone(*ptr),
            },
            ExpressionKind::SIMDLoadStoreLane {
                is_store,
                op,
                offset,
                align,
                index,
                ptr,
                vec,
            } => ExpressionKind::SIMDLoadStoreLane {
                is_store: *is_store,
                op: *op,
                offset: *offset,
                align: *align,
                index: *index,
                ptr: builder.deep_clone(*ptr),
                vec: builder.deep_clone(*vec),
            },
            ExpressionKind::MemoryInit {
                memory,
                segment,
                dest,
                offset,
                size,
            } => ExpressionKind::MemoryInit {
                memory: *memory,
                segment: *segment,
                dest: builder.deep_clone(*dest),
                offset: builder.deep_clone(*offset),
                size: builder.deep_clone(*size),
            },
            ExpressionKind::DataDrop { segment } => ExpressionKind::DataDrop { segment: *segment },
            ExpressionKind::MemoryCopy {
                dest_memory,
                src_memory,
                dest,
                src,
                size,
            } => ExpressionKind::MemoryCopy {
                dest_memory: *dest_memory,
                src_memory: *src_memory,
                dest: builder.deep_clone(*dest),
                src: builder.deep_clone(*src),
                size: builder.deep_clone(*size),
            },
            ExpressionKind::MemoryFill {
                memory,
                dest,
                value,
                size,
            } => ExpressionKind::MemoryFill {
                memory: *memory,
                dest: builder.deep_clone(*dest),
                value: builder.deep_clone(*value),
                size: builder.deep_clone(*size),
            },
            ExpressionKind::TableGet { table, index } => ExpressionKind::TableGet {
                table: *table,
                index: builder.deep_clone(*index),
            },
            ExpressionKind::TableSet {
                table,
                index,
                value,
            } => ExpressionKind::TableSet {
                table: *table,
                index: builder.deep_clone(*index),
                value: builder.deep_clone(*value),
            },
            ExpressionKind::TableSize { table } => ExpressionKind::TableSize { table: *table },
            ExpressionKind::TableGrow {
                table,
                delta,
                value,
            } => ExpressionKind::TableGrow {
                table: *table,
                delta: builder.deep_clone(*delta),
                value: builder.deep_clone(*value),
            },
            ExpressionKind::TableFill {
                table,
                dest,
                value,
                size,
            } => ExpressionKind::TableFill {
                table: *table,
                dest: builder.deep_clone(*dest),
                value: builder.deep_clone(*value),
                size: builder.deep_clone(*size),
            },
            ExpressionKind::TableCopy {
                dest_table,
                src_table,
                dest,
                src,
                size,
            } => ExpressionKind::TableCopy {
                dest_table: *dest_table,
                src_table: *src_table,
                dest: builder.deep_clone(*dest),
                src: builder.deep_clone(*src),
                size: builder.deep_clone(*size),
            },
            ExpressionKind::TableInit {
                table,
                segment,
                dest,
                offset,
                size,
            } => ExpressionKind::TableInit {
                table: *table,
                segment: *segment,
                dest: builder.deep_clone(*dest),
                offset: builder.deep_clone(*offset),
                size: builder.deep_clone(*size),
            },
            ExpressionKind::RefNull { type_ } => ExpressionKind::RefNull { type_: *type_ },
            ExpressionKind::RefIsNull { value } => ExpressionKind::RefIsNull {
                value: builder.deep_clone(*value),
            },
            ExpressionKind::RefAs { op, value } => ExpressionKind::RefAs {
                op: *op,
                value: builder.deep_clone(*value),
            },
            ExpressionKind::RefEq { left, right } => ExpressionKind::RefEq {
                left: builder.deep_clone(*left),
                right: builder.deep_clone(*right),
            },
            ExpressionKind::RefFunc { func } => ExpressionKind::RefFunc { func: *func },
            ExpressionKind::StructNew { type_, operands } => ExpressionKind::StructNew {
                type_: *type_,
                operands: operands.iter().map(|&c| builder.deep_clone(c)).collect(),
            },
            ExpressionKind::StructGet {
                index,
                ptr,
                type_,
                signed,
            } => ExpressionKind::StructGet {
                index: *index,
                ptr: builder.deep_clone(*ptr),
                type_: *type_,
                signed: *signed,
            },
            ExpressionKind::StructSet {
                index,
                ptr,
                value,
                type_,
            } => ExpressionKind::StructSet {
                index: *index,
                ptr: builder.deep_clone(*ptr),
                value: builder.deep_clone(*value),
                type_: *type_,
            },
            ExpressionKind::ArrayNew { type_, size, init } => ExpressionKind::ArrayNew {
                type_: *type_,
                size: builder.deep_clone(*size),
                init: init.map(|i| builder.deep_clone(i)),
            },
            ExpressionKind::ArrayGet {
                ptr,
                index,
                type_,
                signed,
            } => ExpressionKind::ArrayGet {
                ptr: builder.deep_clone(*ptr),
                index: builder.deep_clone(*index),
                type_: *type_,
                signed: *signed,
            },
            ExpressionKind::ArraySet {
                ptr,
                index,
                value,
                type_,
            } => ExpressionKind::ArraySet {
                ptr: builder.deep_clone(*ptr),
                index: builder.deep_clone(*index),
                value: builder.deep_clone(*value),
                type_: *type_,
            },
            ExpressionKind::ArrayLen { ptr } => ExpressionKind::ArrayLen {
                ptr: builder.deep_clone(*ptr),
            },
            ExpressionKind::ElemDrop { segment } => ExpressionKind::ElemDrop { segment: *segment },
            ExpressionKind::Try {
                name,
                body,
                catch_tags,
                catch_bodies,
                delegate,
            } => ExpressionKind::Try {
                name: name.clone(),
                body: builder.deep_clone(*body),
                catch_tags: catch_tags.clone(),
                catch_bodies: catch_bodies
                    .iter()
                    .map(|&c| builder.deep_clone(c))
                    .collect(),
                delegate: delegate.clone(),
            },
            ExpressionKind::Throw { tag, operands } => ExpressionKind::Throw {
                tag: tag.clone(),
                operands: operands.iter().map(|&c| builder.deep_clone(c)).collect(),
            },
            ExpressionKind::Rethrow { target } => ExpressionKind::Rethrow {
                target: target.clone(),
            },
            ExpressionKind::TupleMake { operands } => ExpressionKind::TupleMake {
                operands: operands.iter().map(|&c| builder.deep_clone(c)).collect(),
            },
            ExpressionKind::TupleExtract { index, tuple } => ExpressionKind::TupleExtract {
                index: *index,
                tuple: builder.deep_clone(*tuple),
            },
            ExpressionKind::Pop { type_ } => ExpressionKind::Pop { type_: *type_ },
            ExpressionKind::I31New { value } => ExpressionKind::I31New {
                value: builder.deep_clone(*value),
            },
            ExpressionKind::I31Get { i31, signed } => ExpressionKind::I31Get {
                i31: builder.deep_clone(*i31),
                signed: *signed,
            },
            ExpressionKind::RefTest { op, value, type_ } => ExpressionKind::RefTest {
                op: *op,
                value: builder.deep_clone(*value),
                type_: *type_,
            },
            ExpressionKind::RefCast { op, value, type_ } => ExpressionKind::RefCast {
                op: *op,
                value: builder.deep_clone(*value),
                type_: *type_,
            },
            ExpressionKind::BrOn {
                target,
                op,
                value,
                type_,
            } => ExpressionKind::BrOn {
                target: *target,
                op: *op,
                value: builder.deep_clone(*value),
                type_: *type_,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Literal, Type};

    #[test]
    fn test_expression_nop() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);
        let nop_id = builder.nop();
        let nop = &arena[nop_id];
        assert!(matches!(nop.kind, ExpressionKind::Nop));
        assert_eq!(nop.type_, Type::NONE);
    }

    #[test]
    fn test_expression_const() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);
        let const_id = builder.const_(Literal::I32(42));
        let const_expr = &arena[const_id];
        assert!(matches!(
            const_expr.kind,
            ExpressionKind::Const(Literal::I32(42))
        ));
        assert_eq!(const_expr.type_, Type::I32);
    }

    #[test]
    fn test_expression_block() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);
        let nop_id = builder.nop();
        let const_id = builder.const_(Literal::I32(1));

        let block_id = builder.block(None::<String>, vec![nop_id, const_id], Type::I32);
        let block = &arena[block_id];
        assert!(matches!(block.kind, ExpressionKind::Block { .. }));
        assert_eq!(block.type_, Type::I32);

        if let ExpressionKind::Block { list, .. } = &block.kind {
            assert_eq!(list.len(), 2);
        }
    }

    #[test]
    fn test_expression_local_set() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);
        let val = builder.const_(Literal::I32(10));
        let set_id = builder.local_set(LocalId::from(0), val);
        let set = &arena[set_id];

        assert!(matches!(
            set.kind,
            ExpressionKind::LocalSet { id, .. } if id.index() == 0
        ));
        assert_eq!(set.type_, Type::NONE);
    }

    #[test]
    fn test_expression_local_get() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);
        let get_id = builder.local_get(LocalId::from(0), Type::I32);
        let get = &arena[get_id];

        assert!(matches!(get.kind, ExpressionKind::LocalGet { id } if id.index() == 0));
        assert_eq!(get.type_, Type::I32);
    }

    #[test]
    fn test_expression_local_tee() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);
        let val = builder.const_(Literal::I32(5));
        let tee_id = builder.local_tee(LocalId::from(0), val, Type::I32);
        let tee = &arena[tee_id];

        assert!(matches!(
            tee.kind,
            ExpressionKind::LocalTee { id, .. } if id.index() == 0
        ));
        assert_eq!(tee.type_, Type::I32);
    }

    #[test]
    fn test_expression_helpers_integration() {
        let mut arena = IdArena::new();
        let mut builder = IrBuilder::new(&mut arena);

        // Build: (block (local.set $0 (i32.const 42)) (local.get $0))
        let const_val = builder.const_(Literal::I32(42));
        let set = builder.local_set(LocalId::from(0), const_val);
        let get = builder.local_get(LocalId::from(0), Type::I32);

        let block_id = builder.block(None::<String>, vec![set, get], Type::I32);
        let block = &arena[block_id];

        // Verify structure
        if let ExpressionKind::Block { list, .. } = &block.kind {
            assert_eq!(list.len(), 2);
            assert!(matches!(
                arena[list[0]].kind,
                ExpressionKind::LocalSet { .. }
            ));
            assert!(matches!(
                arena[list[1]].kind,
                ExpressionKind::LocalGet { .. }
            ));
        } else {
            panic!("Expected Block");
        }
    }
}
