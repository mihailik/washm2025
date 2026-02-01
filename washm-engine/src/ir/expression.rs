use crate::ir::ops::{BinaryOp, UnaryOp};
use crate::ir::{Literal, Type, GlobalId};
use bumpalo::collections::Vec as BumpVec;
use bumpalo::Bump;
use std::ops::{Deref, DerefMut};
use std::ptr::NonNull;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct ExprRef<'a>(NonNull<Expression<'a>>);

impl<'a> ExprRef<'a> {
    pub fn new(ptr: &'a mut Expression<'a>) -> Self {
        Self(NonNull::from(ptr))
    }

    pub fn as_ptr(&self) -> *mut Expression<'a> {
        self.0.as_ptr()
    }
}

unsafe impl<'a> Send for ExprRef<'a> {}
unsafe impl<'a> Sync for ExprRef<'a> {}

impl<'a> Deref for ExprRef<'a> {
    type Target = Expression<'a>;
    fn deref(&self) -> &Self::Target {
        unsafe { self.0.as_ref() }
    }
}

impl<'a> DerefMut for ExprRef<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.0.as_mut() }
    }
}

#[derive(Debug)]
pub struct Expression<'a> {
    pub type_: Type,
    pub kind: ExpressionKind<'a>,
}

#[derive(Debug)]
pub enum ExpressionKind<'a> {
    Block {
        name: Option<&'a str>,
        list: BumpVec<'a, ExprRef<'a>>,
    },
    Const(Literal),
    Unary {
        op: UnaryOp,
        value: ExprRef<'a>,
    },
    Binary {
        op: BinaryOp,
        left: ExprRef<'a>,
        right: ExprRef<'a>,
    },
    Call {
        target: &'a str,
        operands: BumpVec<'a, ExprRef<'a>>,
        is_return: bool,
    },
    LocalGet {
        index: u32,
    },
    LocalSet {
        index: u32,
        value: ExprRef<'a>,
    },
    LocalTee {
        index: u32,
        value: ExprRef<'a>,
    },
    GlobalGet {
        id: GlobalId,
    },
    GlobalSet {
        id: GlobalId,
        value: ExprRef<'a>,
    },
    If {
        condition: ExprRef<'a>,
        if_true: ExprRef<'a>,
        if_false: Option<ExprRef<'a>>,
    },
    Loop {
        name: Option<&'a str>,
        body: ExprRef<'a>,
    },
    Break {
        name: &'a str,
        condition: Option<ExprRef<'a>>,
        value: Option<ExprRef<'a>>,
    },
    Return {
        value: Option<ExprRef<'a>>,
    },
    Unreachable,
    Drop {
        value: ExprRef<'a>,
    },
    Select {
        condition: ExprRef<'a>,
        if_true: ExprRef<'a>,
        if_false: ExprRef<'a>,
    },
    Load {
        bytes: u32,       // 1, 2, 4, or 8
        signed: bool,     // For sub-word loads
        offset: u32,      // Memory offset
        align: u32,       // Alignment (power of 2)
        ptr: ExprRef<'a>, // Address to load from
    },
    Store {
        bytes: u32,         // 1, 2, 4, or 8
        offset: u32,        // Memory offset
        align: u32,         // Alignment
        ptr: ExprRef<'a>,   // Address to store to
        value: ExprRef<'a>, // Value to store
    },
    Nop,
    Switch {
        names: BumpVec<'a, &'a str>, // List of target labels
        default: &'a str,            // Default label
        condition: ExprRef<'a>,      // Index
        value: Option<ExprRef<'a>>,  // Value passing (nullable)
    },
    CallIndirect {
        table: &'a str,
        target: ExprRef<'a>, // Function index
        operands: BumpVec<'a, ExprRef<'a>>,
        type_: Type, // Signature
    },
    MemoryGrow {
        delta: ExprRef<'a>,
    },
    MemorySize,
    // Atomic operations
    AtomicRMW {
        op: crate::ir::ops::AtomicOp,
        bytes: u32,
        offset: u32,
        ptr: ExprRef<'a>,
        value: ExprRef<'a>,
    },
    AtomicCmpxchg {
        bytes: u32,
        offset: u32,
        ptr: ExprRef<'a>,
        expected: ExprRef<'a>,
        replacement: ExprRef<'a>,
    },
    AtomicWait {
        ptr: ExprRef<'a>,
        expected: ExprRef<'a>,
        timeout: ExprRef<'a>,
        expected_type: Type,
    },
    AtomicNotify {
        ptr: ExprRef<'a>,
        count: ExprRef<'a>,
    },
    AtomicFence,
    // Tuple operations
    TupleMake {
        operands: BumpVec<'a, ExprRef<'a>>,
    },
    TupleExtract {
        tuple: ExprRef<'a>,
        index: u32,
    },
    // Reference types
    RefNull {
        type_: Type,
    },
    RefIsNull {
        value: ExprRef<'a>,
    },
    RefFunc {
        func: &'a str,
    },
    RefEq {
        left: ExprRef<'a>,
        right: ExprRef<'a>,
    },
    RefAs {
        op: crate::ir::ops::RefAsOp,
        value: ExprRef<'a>,
    },
    TableGet {
        table: &'a str,
        index: ExprRef<'a>,
    },
    TableSet {
        table: &'a str,
        index: ExprRef<'a>,
        value: ExprRef<'a>,
    },
    TableSize {
        table: &'a str,
    },
    TableGrow {
        table: &'a str,
        value: ExprRef<'a>,
        delta: ExprRef<'a>,
    },
    TableFill {
        table: &'a str,
        dest: ExprRef<'a>,
        value: ExprRef<'a>,
        size: ExprRef<'a>,
    },
    TableCopy {
        dest_table: &'a str,
        src_table: &'a str,
        dest: ExprRef<'a>,
        src: ExprRef<'a>,
        size: ExprRef<'a>,
    },
    TableInit {
        table: &'a str,
        segment: u32,
        dest: ExprRef<'a>,
        offset: ExprRef<'a>,
        size: ExprRef<'a>,
    },
    ElemDrop {
        segment: u32,
    },
    MemoryInit {
        segment: u32,
        dest: ExprRef<'a>,
        offset: ExprRef<'a>,
        size: ExprRef<'a>,
    },
    DataDrop {
        segment: u32,
    },
    MemoryCopy {
        dest: ExprRef<'a>,
        src: ExprRef<'a>,
        size: ExprRef<'a>,
    },
    MemoryFill {
        dest: ExprRef<'a>,
        value: ExprRef<'a>,
        size: ExprRef<'a>,
    },
    Pop {
        type_: Type,
    },
    I31New {
        value: ExprRef<'a>,
    },
    I31Get {
        i31: ExprRef<'a>,
        signed: bool,
    },
    SIMDExtract {
        op: crate::ir::ops::SIMDExtractOp,
        vec: ExprRef<'a>,
        index: u8,
    },
    SIMDReplace {
        op: crate::ir::ops::SIMDReplaceOp,
        vec: ExprRef<'a>,
        index: u8,
        value: ExprRef<'a>,
    },
    SIMDShuffle {
        left: ExprRef<'a>,
        right: ExprRef<'a>,
        mask: [u8; 16],
    },
    SIMDTernary {
        op: crate::ir::ops::SIMDTernaryOp,
        a: ExprRef<'a>,
        b: ExprRef<'a>,
        c: ExprRef<'a>,
    },
    SIMDShift {
        op: crate::ir::ops::SIMDShiftOp,
        vec: ExprRef<'a>,
        shift: ExprRef<'a>,
    },
    SIMDLoad {
        op: crate::ir::ops::SIMDLoadOp,
        offset: u32,
        align: u32,
        ptr: ExprRef<'a>,
    },
    SIMDLoadStoreLane {
        op: crate::ir::ops::SIMDLoadStoreLaneOp,
        offset: u32,
        align: u32,
        index: u8,
        is_store: bool,
        ptr: ExprRef<'a>,
        vec: ExprRef<'a>,
    },
    StructNew {
        type_: Type, // Heap type
        operands: BumpVec<'a, ExprRef<'a>>,
    },
    StructGet {
        type_: Type, // Heap type
        ptr: ExprRef<'a>,
        index: u32,
        signed: bool,
    },
    StructSet {
        type_: Type, // Heap type
        ptr: ExprRef<'a>,
        index: u32,
        value: ExprRef<'a>,
    },
    ArrayNew {
        type_: Type, // Heap type
        size: ExprRef<'a>,
        init: Option<ExprRef<'a>>,
    },
    ArrayGet {
        type_: Type, // Heap type
        ptr: ExprRef<'a>,
        index: ExprRef<'a>,
        signed: bool,
    },
    ArraySet {
        type_: Type, // Heap type
        ptr: ExprRef<'a>,
        index: ExprRef<'a>,
        value: ExprRef<'a>,
    },
    ArrayLen {
        ptr: ExprRef<'a>,
    },
    Try {
        name: Option<&'a str>,
        body: ExprRef<'a>,
        catch_tags: BumpVec<'a, &'a str>,
        catch_bodies: BumpVec<'a, ExprRef<'a>>,
        delegate: Option<&'a str>,
    },
    Throw {
        tag: &'a str,
        operands: BumpVec<'a, ExprRef<'a>>,
    },
    Rethrow {
        target: &'a str,
    },
    RefTest {
        op: crate::ir::ops::RefTestOp,
        value: ExprRef<'a>,
        type_: Type,
    },
    RefCast {
        op: crate::ir::ops::RefCastOp,
        value: ExprRef<'a>,
        type_: Type,
    },
    BrOn {
        name: &'a str,
        op: crate::ir::ops::BrOnOp,
        value: ExprRef<'a>,
        type_: Type,
    },
}

impl<'a> ExpressionKind<'a> {
    pub fn for_each_child<F>(&self, mut f: F)
    where
        F: FnMut(ExprRef<'a>),
    {
        match self {
            ExpressionKind::Unary { value, .. }
            | ExpressionKind::LocalSet { value, .. }
            | ExpressionKind::LocalTee { value, .. }
            | ExpressionKind::GlobalSet { value, .. }
            | ExpressionKind::Drop { value }
            | ExpressionKind::Load { ptr: value, .. }
            | ExpressionKind::MemoryGrow { delta: value }
            | ExpressionKind::RefIsNull { value }
            | ExpressionKind::RefAs { value, .. }
            | ExpressionKind::RefTest { value, .. }
            | ExpressionKind::RefCast { value, .. }
            | ExpressionKind::BrOn { value, .. }
            | ExpressionKind::I31New { value }
            | ExpressionKind::I31Get { i31: value, .. }
            | ExpressionKind::TupleExtract { tuple: value, .. }
            | ExpressionKind::SIMDExtract { vec: value, .. }
            | ExpressionKind::SIMDShift { vec: value, .. }
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
            } => {
                f(*left);
                f(*right);
            }
            ExpressionKind::TableGet { table: _, index } => {
                f(*index);
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
            ExpressionKind::If {
                condition,
                if_true,
                if_false,
            } => {
                f(*condition);
                f(*if_true);
                if let Some(if_false) = if_false {
                    f(*if_false);
                }
            }
            ExpressionKind::ArrayNew { size, init, .. } => {
                f(*size);
                if let Some(init) = init {
                    f(*init);
                }
            }
            ExpressionKind::ArraySet {
                ptr, index, value, ..
            } => {
                f(*ptr);
                f(*index);
                f(*value);
            }
            ExpressionKind::Block { list, .. }
            | ExpressionKind::TupleMake { operands: list }
            | ExpressionKind::Call { operands: list, .. }
            | ExpressionKind::StructNew { operands: list, .. }
            | ExpressionKind::Throw { operands: list, .. } => {
                for &child in list {
                    f(child);
                }
            }
            ExpressionKind::CallIndirect {
                target, operands, ..
            } => {
                f(*target);
                for &child in operands {
                    f(child);
                }
            }
            ExpressionKind::Switch {
                condition, value, ..
            } => {
                f(*condition);
                if let Some(value) = value {
                    f(*value);
                }
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
            ExpressionKind::MemoryCopy { dest, src, size }
            | ExpressionKind::TableCopy {
                dest, src, size, ..
            } => {
                f(*dest);
                f(*src);
                f(*size);
            }
            ExpressionKind::TableFill {
                dest, value, size, ..
            }
            | ExpressionKind::MemoryFill {
                dest, value, size, ..
            } => {
                f(*dest);
                f(*value);
                f(*size);
            }
            ExpressionKind::TableGrow { value, delta, .. } => {
                f(*value);
                f(*delta);
            }
            ExpressionKind::Try {
                body, catch_bodies, ..
            } => {
                f(*body);
                for &catch_body in catch_bodies {
                    f(catch_body);
                }
            }
            ExpressionKind::Loop { body, .. } => {
                f(*body);
            }
            ExpressionKind::Pop { .. }
            | ExpressionKind::Const(_)
            | ExpressionKind::LocalGet { .. }
            | ExpressionKind::GlobalGet { .. }
            | ExpressionKind::Unreachable
            | ExpressionKind::AtomicFence
            | ExpressionKind::RefNull { .. }
            | ExpressionKind::RefFunc { .. }
            | ExpressionKind::TableSize { .. }
            | ExpressionKind::MemorySize
            | ExpressionKind::DataDrop { .. }
            | ExpressionKind::ElemDrop { .. }
            | ExpressionKind::Nop
            | ExpressionKind::Rethrow { .. }
            | ExpressionKind::Break {
                condition: None,
                value: None,
                ..
            } => {}
            ExpressionKind::Break {
                condition, value, ..
            } => {
                if let Some(cond) = condition {
                    f(*cond);
                }
                if let Some(val) = value {
                    f(*val);
                }
            }
        }
    }
}
impl<'a> Expression<'a> {
    /// Recompute the type of the expression based on its children
    pub fn finalize(&mut self) {
        match &mut self.kind {
            ExpressionKind::Block { list, .. } => {
                if let Some(last) = list.last() {
                    self.type_ = last.type_;
                } else {
                    self.type_ = Type::NONE;
                }
            }
            ExpressionKind::If {
                if_true, if_false, ..
            } => {
                if let Some(false_branch) = if_false {
                    if if_true.type_ == false_branch.type_ {
                        self.type_ = if_true.type_;
                    } else if if_true.type_ == Type::UNREACHABLE {
                        self.type_ = false_branch.type_;
                    } else if false_branch.type_ == Type::UNREACHABLE {
                        self.type_ = if_true.type_;
                    } else {
                        self.type_ = Type::NONE;
                    }
                } else {
                    self.type_ = Type::NONE;
                }
            }
            ExpressionKind::Loop { body, .. } => {
                self.type_ = body.type_;
            }
            ExpressionKind::LocalSet { .. }
            | ExpressionKind::GlobalSet { .. }
            | ExpressionKind::Store { .. }
            | ExpressionKind::Drop { .. }
            | ExpressionKind::Nop => {
                self.type_ = Type::NONE;
            }
            ExpressionKind::Const(lit) => {
                self.type_ = lit.get_type();
            }
            ExpressionKind::LocalTee { value, .. } => {
                self.type_ = value.type_;
            }
            ExpressionKind::LocalGet { .. } | ExpressionKind::GlobalGet { .. } => {
                // type_ should already be correct, or set at creation
            }
            ExpressionKind::Return { .. } | ExpressionKind::Unreachable => {
                self.type_ = Type::UNREACHABLE;
            }
            _ => {
                // For other expressions, keep the current type or refine if possible
            }
        }
    }
}

impl<'a> Expression<'a> {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(bump: &'a Bump, kind: ExpressionKind<'a>, type_: Type) -> ExprRef<'a> {
        ExprRef(NonNull::from(bump.alloc(Expression { kind, type_ })))
    }

    /// Create a new nop expression
    pub fn nop(bump: &'a Bump) -> ExprRef<'a> {
        ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Nop,
            type_: Type::NONE,
        }))
    }

    /// Create a new const expression
    pub fn const_expr(bump: &'a Bump, lit: Literal, ty: Type) -> ExprRef<'a> {
        ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Const(lit),
            type_: ty,
        }))
    }

    /// Create a new block
    pub fn block(
        bump: &'a Bump,
        name: Option<&'a str>,
        list: BumpVec<'a, ExprRef<'a>>,
        ty: Type,
    ) -> ExprRef<'a> {
        ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::Block { name, list },
            type_: ty,
        }))
    }

    /// Create local.set
    pub fn local_set(bump: &'a Bump, index: u32, value: ExprRef<'a>) -> ExprRef<'a> {
        ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::LocalSet { index, value },
            type_: Type::NONE,
        }))
    }

    /// Create local.get
    pub fn local_get(bump: &'a Bump, index: u32, ty: Type) -> ExprRef<'a> {
        ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::LocalGet { index },
            type_: ty,
        }))
    }

    /// Create local.tee
    pub fn local_tee(bump: &'a Bump, index: u32, value: ExprRef<'a>, ty: Type) -> ExprRef<'a> {
        ExprRef::new(bump.alloc(Expression {
            kind: ExpressionKind::LocalTee { index, value },
            type_: ty,
        }))
    }
}

// Helpers for construction
#[derive(Clone, Copy)]
pub struct IrBuilder<'a> {
    pub bump: &'a Bump,
}

impl<'a> IrBuilder<'a> {
    pub fn new(bump: &'a Bump) -> Self {
        Self { bump }
    }

    pub fn nop(&self) -> ExprRef<'a> {
        Expression::new(self.bump, ExpressionKind::Nop, Type::NONE)
    }

    pub fn const_(&self, value: Literal) -> ExprRef<'a> {
        let type_ = value.get_type();
        Expression::new(self.bump, ExpressionKind::Const(value), type_)
    }

    pub fn block(
        &self,
        name: Option<&'a str>,
        list: BumpVec<'a, ExprRef<'a>>,
        type_: Type,
    ) -> ExprRef<'a> {
        Expression::new(self.bump, ExpressionKind::Block { name, list }, type_)
    }

    pub fn unary(&self, op: UnaryOp, value: ExprRef<'a>, type_: Type) -> ExprRef<'a> {
        Expression::new(self.bump, ExpressionKind::Unary { op, value }, type_)
    }

    pub fn binary(
        &self,
        op: BinaryOp,
        left: ExprRef<'a>,
        right: ExprRef<'a>,
        type_: Type,
    ) -> ExprRef<'a> {
        Expression::new(self.bump, ExpressionKind::Binary { op, left, right }, type_)
    }

    pub fn drop(&self, value: ExprRef<'a>) -> ExprRef<'a> {
        Expression::new(self.bump, ExpressionKind::Drop { value }, Type::NONE)
    }

    pub fn call(
        &self,
        target: &'a str,
        operands: BumpVec<'a, ExprRef<'a>>,
        type_: Type,
        is_return: bool,
    ) -> ExprRef<'a> {
        Expression::new(
            self.bump,
            ExpressionKind::Call {
                target,
                operands,
                is_return,
            },
            type_,
        )
    }

    pub fn call_indirect(
        &self,
        table: &'a str,
        target: ExprRef<'a>,
        operands: BumpVec<'a, ExprRef<'a>>,
        sig: Type,
        type_: Type,
    ) -> ExprRef<'a> {
        Expression::new(
            self.bump,
            ExpressionKind::CallIndirect {
                table,
                target,
                operands,
                type_: sig,
            },
            type_,
        )
    }

    pub fn local_get(&self, index: u32, type_: Type) -> ExprRef<'a> {
        Expression::new(self.bump, ExpressionKind::LocalGet { index }, type_)
    }

    pub fn local_set(&self, index: u32, value: ExprRef<'a>) -> ExprRef<'a> {
        Expression::new(
            self.bump,
            ExpressionKind::LocalSet { index, value },
            Type::NONE,
        )
    }

    pub fn local_tee(&self, index: u32, value: ExprRef<'a>, type_: Type) -> ExprRef<'a> {
        Expression::new(self.bump, ExpressionKind::LocalTee { index, value }, type_)
    }

    pub fn global_get(&self, id: GlobalId, type_: Type) -> ExprRef<'a> {
        Expression::new(self.bump, ExpressionKind::GlobalGet { id: id.cast() }, type_)
    }

    pub fn global_set(&self, id: GlobalId, value: ExprRef<'a>) -> ExprRef<'a> {
        Expression::new(
            self.bump,
            ExpressionKind::GlobalSet { id: id.cast(), value },
            Type::NONE,
        )
    }

    pub fn switch(
        &self,
        names: BumpVec<'a, &'a str>,
        default: &'a str,
        condition: ExprRef<'a>,
        value: Option<ExprRef<'a>>,
    ) -> ExprRef<'a> {
        Expression::new(
            self.bump,
            ExpressionKind::Switch {
                names,
                default,
                condition,
                value,
            },
            Type::UNREACHABLE,
        )
    }

    pub fn if_(
        &self,
        condition: ExprRef<'a>,
        if_true: ExprRef<'a>,
        if_false: Option<ExprRef<'a>>,
        type_: Type,
    ) -> ExprRef<'a> {
        Expression::new(
            self.bump,
            ExpressionKind::If {
                condition,
                if_true,
                if_false,
            },
            type_,
        )
    }

    pub fn loop_(&self, name: Option<&'a str>, body: ExprRef<'a>, type_: Type) -> ExprRef<'a> {
        Expression::new(self.bump, ExpressionKind::Loop { name, body }, type_)
    }

    pub fn break_(
        &self,
        name: &'a str,
        condition: Option<ExprRef<'a>>,
        value: Option<ExprRef<'a>>,
        type_: Type,
    ) -> ExprRef<'a> {
        Expression::new(
            self.bump,
            ExpressionKind::Break {
                name,
                condition,
                value,
            },
            type_,
        )
    }

    pub fn load(
        &self,
        bytes: u32,
        signed: bool,
        offset: u32,
        align: u32,
        ptr: ExprRef<'a>,
        type_: Type,
    ) -> ExprRef<'a> {
        Expression::new(
            self.bump,
            ExpressionKind::Load {
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
        &self,
        bytes: u32,
        offset: u32,
        align: u32,
        ptr: ExprRef<'a>,
        value: ExprRef<'a>,
    ) -> ExprRef<'a> {
        Expression::new(
            self.bump,
            ExpressionKind::Store {
                bytes,
                offset,
                align,
                ptr,
                value,
            },
            Type::NONE,
        )
    }

    pub fn return_(&self, value: Option<ExprRef<'a>>) -> ExprRef<'a> {
        Expression::new(
            self.bump,
            ExpressionKind::Return { value },
            Type::UNREACHABLE,
        )
    }

    pub fn unreachable(&self) -> ExprRef<'a> {
        Expression::new(self.bump, ExpressionKind::Unreachable, Type::UNREACHABLE)
    }

    pub fn select(
        &self,
        condition: ExprRef<'a>,
        if_true: ExprRef<'a>,
        if_false: ExprRef<'a>,
        type_: Type,
    ) -> ExprRef<'a> {
        Expression::new(
            self.bump,
            ExpressionKind::Select {
                condition,
                if_true,
                if_false,
            },
            type_,
        )
    }

    pub fn memory_size(&self) -> ExprRef<'a> {
        Expression::new(self.bump, ExpressionKind::MemorySize, Type::I32)
    }

    pub fn memory_grow(&self, delta: ExprRef<'a>) -> ExprRef<'a> {
        Expression::new(self.bump, ExpressionKind::MemoryGrow { delta }, Type::I32)
    }

    pub fn atomic_rmw(
        &self,
        op: crate::ir::ops::AtomicOp,
        bytes: u32,
        offset: u32,
        ptr: ExprRef<'a>,
        value: ExprRef<'a>,
        type_: Type,
    ) -> ExprRef<'a> {
        Expression::new(
            self.bump,
            ExpressionKind::AtomicRMW {
                op,
                bytes,
                offset,
                ptr,
                value,
            },
            type_,
        )
    }

    pub fn atomic_cmpxchg(
        &self,
        bytes: u32,
        offset: u32,
        ptr: ExprRef<'a>,
        expected: ExprRef<'a>,
        replacement: ExprRef<'a>,
        type_: Type,
    ) -> ExprRef<'a> {
        Expression::new(
            self.bump,
            ExpressionKind::AtomicCmpxchg {
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
        &self,
        ptr: ExprRef<'a>,
        expected: ExprRef<'a>,
        timeout: ExprRef<'a>,
        expected_type: Type,
    ) -> ExprRef<'a> {
        Expression::new(
            self.bump,
            ExpressionKind::AtomicWait {
                ptr,
                expected,
                timeout,
                expected_type,
            },
            Type::I32,
        )
    }

    pub fn atomic_notify(&self, ptr: ExprRef<'a>, count: ExprRef<'a>) -> ExprRef<'a> {
        Expression::new(
            self.bump,
            ExpressionKind::AtomicNotify { ptr, count },
            Type::I32,
        )
    }

    pub fn atomic_fence(&self) -> ExprRef<'a> {
        Expression::new(self.bump, ExpressionKind::AtomicFence, Type::NONE)
    }

    pub fn memory_init(
        &self,
        segment: u32,
        dest: ExprRef<'a>,
        offset: ExprRef<'a>,
        size: ExprRef<'a>,
    ) -> ExprRef<'a> {
        Expression::new(
            self.bump,
            ExpressionKind::MemoryInit {
                segment,
                dest,
                offset,
                size,
            },
            Type::NONE,
        )
    }

    pub fn data_drop(&self, segment: u32) -> ExprRef<'a> {
        Expression::new(self.bump, ExpressionKind::DataDrop { segment }, Type::NONE)
    }

    pub fn memory_copy(
        &self,
        dest: ExprRef<'a>,
        src: ExprRef<'a>,
        size: ExprRef<'a>,
    ) -> ExprRef<'a> {
        Expression::new(
            self.bump,
            ExpressionKind::MemoryCopy { dest, src, size },
            Type::NONE,
        )
    }

    pub fn memory_fill(
        &self,
        dest: ExprRef<'a>,
        value: ExprRef<'a>,
        size: ExprRef<'a>,
    ) -> ExprRef<'a> {
        Expression::new(
            self.bump,
            ExpressionKind::MemoryFill { dest, value, size },
            Type::NONE,
        )
    }

    pub fn table_get(&self, table: &'a str, index: ExprRef<'a>, type_: Type) -> ExprRef<'a> {
        Expression::new(self.bump, ExpressionKind::TableGet { table, index }, type_)
    }

    pub fn table_set(&self, table: &'a str, index: ExprRef<'a>, value: ExprRef<'a>) -> ExprRef<'a> {
        Expression::new(
            self.bump,
            ExpressionKind::TableSet {
                table,
                index,
                value,
            },
            Type::NONE,
        )
    }

    pub fn table_size(&self, table: &'a str) -> ExprRef<'a> {
        Expression::new(self.bump, ExpressionKind::TableSize { table }, Type::I32)
    }

    pub fn table_grow(
        &self,
        table: &'a str,
        delta: ExprRef<'a>,
        value: ExprRef<'a>,
    ) -> ExprRef<'a> {
        Expression::new(
            self.bump,
            ExpressionKind::TableGrow {
                table,
                delta,
                value,
            },
            Type::I32,
        )
    }

    pub fn table_fill(
        &self,
        table: &'a str,
        dest: ExprRef<'a>,
        value: ExprRef<'a>,
        size: ExprRef<'a>,
    ) -> ExprRef<'a> {
        Expression::new(
            self.bump,
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
        &self,
        dest_table: &'a str,
        src_table: &'a str,
        dest: ExprRef<'a>,
        src: ExprRef<'a>,
        size: ExprRef<'a>,
    ) -> ExprRef<'a> {
        Expression::new(
            self.bump,
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
        &self,
        table: &'a str,
        segment: u32,
        dest: ExprRef<'a>,
        offset: ExprRef<'a>,
        size: ExprRef<'a>,
    ) -> ExprRef<'a> {
        Expression::new(
            self.bump,
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

    pub fn elem_drop(&self, segment: u32) -> ExprRef<'a> {
        Expression::new(self.bump, ExpressionKind::ElemDrop { segment }, Type::NONE)
    }

    pub fn simd_extract(
        &self,
        op: crate::ir::ops::SIMDExtractOp,
        vec: ExprRef<'a>,
        index: u8,
        type_: Type,
    ) -> ExprRef<'a> {
        Expression::new(
            self.bump,
            ExpressionKind::SIMDExtract { op, vec, index },
            type_,
        )
    }

    pub fn simd_replace(
        &self,
        op: crate::ir::ops::SIMDReplaceOp,
        vec: ExprRef<'a>,
        index: u8,
        value: ExprRef<'a>,
    ) -> ExprRef<'a> {
        Expression::new(
            self.bump,
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
        &self,
        left: ExprRef<'a>,
        right: ExprRef<'a>,
        mask: [u8; 16],
    ) -> ExprRef<'a> {
        Expression::new(
            self.bump,
            ExpressionKind::SIMDShuffle { left, right, mask },
            Type::V128,
        )
    }

    pub fn simd_ternary(
        &self,
        op: crate::ir::ops::SIMDTernaryOp,
        a: ExprRef<'a>,
        b: ExprRef<'a>,
        c: ExprRef<'a>,
    ) -> ExprRef<'a> {
        Expression::new(
            self.bump,
            ExpressionKind::SIMDTernary { op, a, b, c },
            Type::V128,
        )
    }

    pub fn simd_shift(
        &self,
        op: crate::ir::ops::SIMDShiftOp,
        vec: ExprRef<'a>,
        shift: ExprRef<'a>,
    ) -> ExprRef<'a> {
        Expression::new(
            self.bump,
            ExpressionKind::SIMDShift { op, vec, shift },
            Type::V128,
        )
    }

    pub fn simd_load(
        &self,
        op: crate::ir::ops::SIMDLoadOp,
        offset: u32,
        align: u32,
        ptr: ExprRef<'a>,
    ) -> ExprRef<'a> {
        Expression::new(
            self.bump,
            ExpressionKind::SIMDLoad {
                op,
                offset,
                align,
                ptr,
            },
            Type::V128,
        )
    }

    pub fn ref_null(&self, type_: Type) -> ExprRef<'a> {
        Expression::new(self.bump, ExpressionKind::RefNull { type_ }, type_)
    }

    pub fn ref_is_null(&self, value: ExprRef<'a>) -> ExprRef<'a> {
        Expression::new(self.bump, ExpressionKind::RefIsNull { value }, Type::I32)
    }

    pub fn ref_func(&self, func: &'a str) -> ExprRef<'a> {
        Expression::new(self.bump, ExpressionKind::RefFunc { func }, Type::FUNCREF)
    }

    pub fn ref_eq(&self, left: ExprRef<'a>, right: ExprRef<'a>) -> ExprRef<'a> {
        Expression::new(self.bump, ExpressionKind::RefEq { left, right }, Type::I32)
    }

    pub fn i31_new(&self, value: ExprRef<'a>) -> ExprRef<'a> {
        Expression::new(self.bump, ExpressionKind::I31New { value }, Type::I31REF)
    }

    pub fn i31_get(&self, i31: ExprRef<'a>, signed: bool) -> ExprRef<'a> {
        Expression::new(self.bump, ExpressionKind::I31Get { i31, signed }, Type::I32)
    }

    pub fn struct_new(&self, type_: Type, operands: BumpVec<'a, ExprRef<'a>>) -> ExprRef<'a> {
        Expression::new(
            self.bump,
            ExpressionKind::StructNew { type_, operands },
            type_,
        )
    }

    pub fn struct_get(
        &self,
        type_: Type,
        ptr: ExprRef<'a>,
        index: u32,
        signed: bool,
        field_type: Type,
    ) -> ExprRef<'a> {
        Expression::new(
            self.bump,
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
        &self,
        type_: Type,
        ptr: ExprRef<'a>,
        index: u32,
        value: ExprRef<'a>,
    ) -> ExprRef<'a> {
        Expression::new(
            self.bump,
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
        &self,
        type_: Type,
        size: ExprRef<'a>,
        init: Option<ExprRef<'a>>,
    ) -> ExprRef<'a> {
        Expression::new(
            self.bump,
            ExpressionKind::ArrayNew { type_, size, init },
            type_,
        )
    }

    pub fn array_get(
        &self,
        type_: Type,
        ptr: ExprRef<'a>,
        index: ExprRef<'a>,
        signed: bool,
        field_type: Type,
    ) -> ExprRef<'a> {
        Expression::new(
            self.bump,
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
        &self,
        type_: Type,
        ptr: ExprRef<'a>,
        index: ExprRef<'a>,
        value: ExprRef<'a>,
    ) -> ExprRef<'a> {
        Expression::new(
            self.bump,
            ExpressionKind::ArraySet {
                type_,
                ptr,
                index,
                value,
            },
            Type::NONE,
        )
    }

    pub fn array_len(&self, ptr: ExprRef<'a>) -> ExprRef<'a> {
        Expression::new(self.bump, ExpressionKind::ArrayLen { ptr }, Type::I32)
    }

    pub fn ref_test(
        &self,
        op: crate::ir::ops::RefTestOp,
        value: ExprRef<'a>,
        type_: Type,
    ) -> ExprRef<'a> {
        Expression::new(
            self.bump,
            ExpressionKind::RefTest { op, value, type_ },
            Type::I32,
        )
    }

    pub fn ref_cast(
        &self,
        op: crate::ir::ops::RefCastOp,
        value: ExprRef<'a>,
        type_: Type,
    ) -> ExprRef<'a> {
        Expression::new(
            self.bump,
            ExpressionKind::RefCast { op, value, type_ },
            type_,
        )
    }

    pub fn br_on(
        &self,
        name: &'a str,
        op: crate::ir::ops::BrOnOp,
        value: ExprRef<'a>,
        type_: Type,
    ) -> ExprRef<'a> {
        Expression::new(
            self.bump,
            ExpressionKind::BrOn {
                name,
                op,
                value,
                type_,
            },
            type_,
        )
    }

    /// Deep clone an expression tree
    pub fn deep_clone(&self, expr: ExprRef<'a>) -> ExprRef<'a> {
        let kind = match &expr.kind {
            ExpressionKind::Block { name, list } => {
                let mut new_list = BumpVec::with_capacity_in(list.len(), self.bump);
                for child in list.iter() {
                    new_list.push(self.deep_clone(*child));
                }
                ExpressionKind::Block {
                    name: *name,
                    list: new_list,
                }
            }
            ExpressionKind::Const(lit) => ExpressionKind::Const(lit.clone()),
            ExpressionKind::Unary { op, value } => ExpressionKind::Unary {
                op: *op,
                value: self.deep_clone(*value),
            },
            ExpressionKind::Binary { op, left, right } => ExpressionKind::Binary {
                op: *op,
                left: self.deep_clone(*left),
                right: self.deep_clone(*right),
            },
            ExpressionKind::Call {
                target,
                operands,
                is_return,
            } => {
                let mut new_operands = BumpVec::with_capacity_in(operands.len(), self.bump);
                for op in operands.iter() {
                    new_operands.push(self.deep_clone(*op));
                }
                ExpressionKind::Call {
                    target,
                    operands: new_operands,
                    is_return: *is_return,
                }
            }
            ExpressionKind::LocalGet { index } => ExpressionKind::LocalGet { index: *index },
            ExpressionKind::LocalSet { index, value } => ExpressionKind::LocalSet {
                index: *index,
                value: self.deep_clone(*value),
            },
            ExpressionKind::LocalTee { index, value } => ExpressionKind::LocalTee {
                index: *index,
                value: self.deep_clone(*value),
            },
            ExpressionKind::GlobalGet { id } => ExpressionKind::GlobalGet { id: *id },
            ExpressionKind::GlobalSet { id, value } => ExpressionKind::GlobalSet {
                id: *id,
                value: self.deep_clone(*value),
            },
            ExpressionKind::If {
                condition,
                if_true,
                if_false,
            } => ExpressionKind::If {
                condition: self.deep_clone(*condition),
                if_true: self.deep_clone(*if_true),
                if_false: if_false.map(|e| self.deep_clone(e)),
            },
            ExpressionKind::Loop { name, body } => ExpressionKind::Loop {
                name: *name,
                body: self.deep_clone(*body),
            },
            ExpressionKind::Break {
                name,
                condition,
                value,
            } => ExpressionKind::Break {
                name,
                condition: condition.map(|e| self.deep_clone(e)),
                value: value.map(|e| self.deep_clone(e)),
            },
            ExpressionKind::Return { value } => ExpressionKind::Return {
                value: value.map(|e| self.deep_clone(e)),
            },
            ExpressionKind::Unreachable => ExpressionKind::Unreachable,
            ExpressionKind::Drop { value } => ExpressionKind::Drop {
                value: self.deep_clone(*value),
            },
            ExpressionKind::Select {
                condition,
                if_true,
                if_false,
            } => ExpressionKind::Select {
                condition: self.deep_clone(*condition),
                if_true: self.deep_clone(*if_true),
                if_false: self.deep_clone(*if_false),
            },
            ExpressionKind::Load {
                bytes,
                signed,
                offset,
                align,
                ptr,
            } => ExpressionKind::Load {
                bytes: *bytes,
                signed: *signed,
                offset: *offset,
                align: *align,
                ptr: self.deep_clone(*ptr),
            },
            ExpressionKind::Store {
                bytes,
                offset,
                align,
                ptr,
                value,
            } => ExpressionKind::Store {
                bytes: *bytes,
                offset: *offset,
                align: *align,
                ptr: self.deep_clone(*ptr),
                value: self.deep_clone(*value),
            },
            ExpressionKind::Nop => ExpressionKind::Nop,
            ExpressionKind::Switch {
                names,
                default,
                condition,
                value,
            } => ExpressionKind::Switch {
                names: names.clone(),
                default,
                condition: self.deep_clone(*condition),
                value: value.map(|e| self.deep_clone(e)),
            },
            ExpressionKind::CallIndirect {
                table,
                target,
                operands,
                type_,
            } => {
                let mut new_operands = BumpVec::with_capacity_in(operands.len(), self.bump);
                for op in operands.iter() {
                    new_operands.push(self.deep_clone(*op));
                }
                ExpressionKind::CallIndirect {
                    table,
                    target: self.deep_clone(*target),
                    operands: new_operands,
                    type_: *type_,
                }
            }
            ExpressionKind::MemoryGrow { delta } => ExpressionKind::MemoryGrow {
                delta: self.deep_clone(*delta),
            },
            ExpressionKind::MemorySize => ExpressionKind::MemorySize,
            ExpressionKind::AtomicRMW {
                op,
                bytes,
                offset,
                ptr,
                value,
            } => ExpressionKind::AtomicRMW {
                op: *op,
                bytes: *bytes,
                offset: *offset,
                ptr: self.deep_clone(*ptr),
                value: self.deep_clone(*value),
            },
            ExpressionKind::AtomicCmpxchg {
                bytes,
                offset,
                ptr,
                expected,
                replacement,
            } => ExpressionKind::AtomicCmpxchg {
                bytes: *bytes,
                offset: *offset,
                ptr: self.deep_clone(*ptr),
                expected: self.deep_clone(*expected),
                replacement: self.deep_clone(*replacement),
            },
            ExpressionKind::AtomicWait {
                ptr,
                expected,
                timeout,
                expected_type,
            } => ExpressionKind::AtomicWait {
                ptr: self.deep_clone(*ptr),
                expected: self.deep_clone(*expected),
                timeout: self.deep_clone(*timeout),
                expected_type: *expected_type,
            },
            ExpressionKind::AtomicNotify { ptr, count } => ExpressionKind::AtomicNotify {
                ptr: self.deep_clone(*ptr),
                count: self.deep_clone(*count),
            },
            ExpressionKind::AtomicFence => ExpressionKind::AtomicFence,
            ExpressionKind::SIMDExtract { op, vec, index } => ExpressionKind::SIMDExtract {
                op: *op,
                vec: self.deep_clone(*vec),
                index: *index,
            },
            ExpressionKind::SIMDReplace {
                op,
                vec,
                index,
                value,
            } => ExpressionKind::SIMDReplace {
                op: *op,
                vec: self.deep_clone(*vec),
                index: *index,
                value: self.deep_clone(*value),
            },
            ExpressionKind::SIMDShuffle { left, right, mask } => ExpressionKind::SIMDShuffle {
                left: self.deep_clone(*left),
                right: self.deep_clone(*right),
                mask: *mask,
            },
            ExpressionKind::SIMDTernary { op, a, b, c } => ExpressionKind::SIMDTernary {
                op: *op,
                a: self.deep_clone(*a),
                b: self.deep_clone(*b),
                c: self.deep_clone(*c),
            },
            ExpressionKind::SIMDShift { op, vec, shift } => ExpressionKind::SIMDShift {
                op: *op,
                vec: self.deep_clone(*vec),
                shift: self.deep_clone(*shift),
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
                ptr: self.deep_clone(*ptr),
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
                ptr: self.deep_clone(*ptr),
                vec: self.deep_clone(*vec),
            },
            ExpressionKind::MemoryInit {
                segment,
                dest,
                offset,
                size,
            } => ExpressionKind::MemoryInit {
                segment: *segment,
                dest: self.deep_clone(*dest),
                offset: self.deep_clone(*offset),
                size: self.deep_clone(*size),
            },
            ExpressionKind::DataDrop { segment } => ExpressionKind::DataDrop { segment: *segment },
            ExpressionKind::MemoryCopy { dest, src, size } => ExpressionKind::MemoryCopy {
                dest: self.deep_clone(*dest),
                src: self.deep_clone(*src),
                size: self.deep_clone(*size),
            },
            ExpressionKind::MemoryFill { dest, value, size } => ExpressionKind::MemoryFill {
                dest: self.deep_clone(*dest),
                value: self.deep_clone(*value),
                size: self.deep_clone(*size),
            },
            ExpressionKind::TableGet { table, index } => ExpressionKind::TableGet {
                table: *table,
                index: self.deep_clone(*index),
            },
            ExpressionKind::TableSet {
                table,
                index,
                value,
            } => ExpressionKind::TableSet {
                table: *table,
                index: self.deep_clone(*index),
                value: self.deep_clone(*value),
            },
            ExpressionKind::TableSize { table } => ExpressionKind::TableSize { table: *table },
            ExpressionKind::TableGrow {
                table,
                delta,
                value,
            } => ExpressionKind::TableGrow {
                table: *table,
                delta: self.deep_clone(*delta),
                value: self.deep_clone(*value),
            },
            ExpressionKind::TableFill {
                table,
                dest,
                value,
                size,
            } => ExpressionKind::TableFill {
                table: *table,
                dest: self.deep_clone(*dest),
                value: self.deep_clone(*value),
                size: self.deep_clone(*size),
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
                dest: self.deep_clone(*dest),
                src: self.deep_clone(*src),
                size: self.deep_clone(*size),
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
                dest: self.deep_clone(*dest),
                offset: self.deep_clone(*offset),
                size: self.deep_clone(*size),
            },
            ExpressionKind::RefNull { type_ } => ExpressionKind::RefNull { type_: *type_ },
            ExpressionKind::RefIsNull { value } => ExpressionKind::RefIsNull {
                value: self.deep_clone(*value),
            },
            ExpressionKind::RefAs { op, value } => ExpressionKind::RefAs {
                op: *op,
                value: self.deep_clone(*value),
            },
            ExpressionKind::RefEq { left, right } => ExpressionKind::RefEq {
                left: self.deep_clone(*left),
                right: self.deep_clone(*right),
            },
            ExpressionKind::RefFunc { func } => ExpressionKind::RefFunc { func: *func },
            ExpressionKind::StructNew { type_, operands } => {
                let mut new_operands = BumpVec::with_capacity_in(operands.len(), self.bump);
                for op in operands.iter() {
                    new_operands.push(self.deep_clone(*op));
                }
                ExpressionKind::StructNew {
                    type_: *type_,
                    operands: new_operands,
                }
            }
            ExpressionKind::StructGet {
                index,
                ptr,
                type_,
                signed,
            } => ExpressionKind::StructGet {
                index: *index,
                ptr: self.deep_clone(*ptr),
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
                ptr: self.deep_clone(*ptr),
                value: self.deep_clone(*value),
                type_: *type_,
            },
            ExpressionKind::ArrayNew { type_, size, init } => ExpressionKind::ArrayNew {
                type_: *type_,
                size: self.deep_clone(*size),
                init: init.map(|i| self.deep_clone(i)),
            },
            ExpressionKind::ArrayGet {
                ptr,
                index,
                type_,
                signed,
            } => ExpressionKind::ArrayGet {
                ptr: self.deep_clone(*ptr),
                index: self.deep_clone(*index),
                type_: *type_,
                signed: *signed,
            },
            ExpressionKind::ArraySet {
                ptr,
                index,
                value,
                type_,
            } => ExpressionKind::ArraySet {
                ptr: self.deep_clone(*ptr),
                index: self.deep_clone(*index),
                value: self.deep_clone(*value),
                type_: *type_,
            },
            ExpressionKind::ArrayLen { ptr } => ExpressionKind::ArrayLen {
                ptr: self.deep_clone(*ptr),
            },
            ExpressionKind::ElemDrop { segment } => ExpressionKind::ElemDrop { segment: *segment },
            ExpressionKind::Try {
                name,
                body,
                catch_tags,
                catch_bodies,
                delegate,
            } => {
                let mut new_catch_bodies = BumpVec::with_capacity_in(catch_bodies.len(), self.bump);
                for b in catch_bodies.iter() {
                    new_catch_bodies.push(self.deep_clone(*b));
                }
                ExpressionKind::Try {
                    name: *name,
                    body: self.deep_clone(*body),
                    catch_tags: catch_tags.clone(),
                    catch_bodies: new_catch_bodies,
                    delegate: *delegate,
                }
            }
            ExpressionKind::Throw { tag, operands } => {
                let mut new_operands = BumpVec::with_capacity_in(operands.len(), self.bump);
                for op in operands.iter() {
                    new_operands.push(self.deep_clone(*op));
                }
                ExpressionKind::Throw {
                    tag: *tag,
                    operands: new_operands,
                }
            }
            ExpressionKind::Rethrow { target } => ExpressionKind::Rethrow { target: *target },
            ExpressionKind::TupleMake { operands } => {
                let mut new_operands = BumpVec::with_capacity_in(operands.len(), self.bump);
                for op in operands.iter() {
                    new_operands.push(self.deep_clone(*op));
                }
                ExpressionKind::TupleMake {
                    operands: new_operands,
                }
            }
            ExpressionKind::TupleExtract { index, tuple } => ExpressionKind::TupleExtract {
                index: *index,
                tuple: self.deep_clone(*tuple),
            },
            ExpressionKind::Pop { type_ } => ExpressionKind::Pop { type_: *type_ },
            ExpressionKind::I31New { value } => ExpressionKind::I31New {
                value: self.deep_clone(*value),
            },
            ExpressionKind::I31Get { i31, signed } => ExpressionKind::I31Get {
                i31: self.deep_clone(*i31),
                signed: *signed,
            },
            ExpressionKind::RefTest { op, value, type_ } => ExpressionKind::RefTest {
                op: *op,
                value: self.deep_clone(*value),
                type_: *type_,
            },
            ExpressionKind::RefCast { op, value, type_ } => ExpressionKind::RefCast {
                op: *op,
                value: self.deep_clone(*value),
                type_: *type_,
            },
            ExpressionKind::BrOn {
                name,
                op,
                value,
                type_,
            } => ExpressionKind::BrOn {
                name,
                op: *op,
                value: self.deep_clone(*value),
                type_: *type_,
            },
        };

        Expression::new(self.bump, kind, expr.type_)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Literal, Type};
    use bumpalo::collections::Vec as BumpVec;
    use bumpalo::Bump;

    #[test]
    fn test_expression_nop() {
        let bump = Bump::new();
        let nop = Expression::nop(&bump);
        assert!(matches!(nop.kind, ExpressionKind::Nop));
        assert_eq!(nop.type_, Type::NONE);
    }

    #[test]
    fn test_expression_const_expr() {
        let bump = Bump::new();
        let const_expr = Expression::const_expr(&bump, Literal::I32(42), Type::I32);
        assert!(matches!(
            const_expr.kind,
            ExpressionKind::Const(Literal::I32(42))
        ));
        assert_eq!(const_expr.type_, Type::I32);
    }

    #[test]
    fn test_expression_block() {
        let bump = Bump::new();
        let mut list = BumpVec::new_in(&bump);
        list.push(Expression::nop(&bump));
        list.push(Expression::const_expr(&bump, Literal::I32(1), Type::I32));

        let block = Expression::block(&bump, None, list, Type::I32);
        assert!(matches!(block.kind, ExpressionKind::Block { .. }));
        assert_eq!(block.type_, Type::I32);

        if let ExpressionKind::Block { list, .. } = &block.kind {
            assert_eq!(list.len(), 2);
        }
    }

    #[test]
    fn test_expression_local_set() {
        let bump = Bump::new();
        let val = Expression::const_expr(&bump, Literal::I32(10), Type::I32);
        let set = Expression::local_set(&bump, 0, val);

        assert!(matches!(
            set.kind,
            ExpressionKind::LocalSet { index: 0, .. }
        ));
        assert_eq!(set.type_, Type::NONE);
    }

    #[test]
    fn test_expression_local_get() {
        let bump = Bump::new();
        let get = Expression::local_get(&bump, 0, Type::I32);

        assert!(matches!(get.kind, ExpressionKind::LocalGet { index: 0 }));
        assert_eq!(get.type_, Type::I32);
    }

    #[test]
    fn test_expression_local_tee() {
        let bump = Bump::new();
        let val = Expression::const_expr(&bump, Literal::I32(5), Type::I32);
        let tee = Expression::local_tee(&bump, 0, val, Type::I32);

        assert!(matches!(
            tee.kind,
            ExpressionKind::LocalTee { index: 0, .. }
        ));
        assert_eq!(tee.type_, Type::I32);
    }

    #[test]
    fn test_expression_helpers_integration() {
        let bump = Bump::new();

        // Build: (block (local.set $0 (i32.const 42)) (local.get $0))
        let const_val = Expression::const_expr(&bump, Literal::I32(42), Type::I32);
        let set = Expression::local_set(&bump, 0, const_val);
        let get = Expression::local_get(&bump, 0, Type::I32);

        let mut list = BumpVec::new_in(&bump);
        list.push(set);
        list.push(get);

        let block = Expression::block(&bump, None, list, Type::I32);

        // Verify structure
        if let ExpressionKind::Block { list, .. } = &block.kind {
            assert_eq!(list.len(), 2);
            assert!(matches!(list[0].kind, ExpressionKind::LocalSet { .. }));
            assert!(matches!(list[1].kind, ExpressionKind::LocalGet { .. }));
        } else {
            panic!("Expected Block");
        }
    }
}
