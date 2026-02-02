#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum UnaryOp {
    ClzInt32,
    CtzInt32,
    PopcntInt32,
    EqZInt32,
    ClzInt64,
    CtzInt64,
    PopcntInt64,
    EqZInt64,
    NegFloat32,
    AbsFloat32,
    CeilFloat32,
    FloorFloat32,
    TruncFloat32,
    NearestFloat32,
    SqrtFloat32,
    NegFloat64,
    AbsFloat64,
    CeilFloat64,
    FloorFloat64,
    TruncFloat64,
    NearestFloat64,
    SqrtFloat64,
    // Conversions (Integer <-> Float)
    ConvertSInt32ToFloat32,
    ConvertUInt32ToFloat32,
    ConvertSInt64ToFloat32,
    ConvertUInt64ToFloat32,
    ConvertSInt32ToFloat64,
    ConvertUInt32ToFloat64,
    ConvertSInt64ToFloat64,
    ConvertUInt64ToFloat64,
    TruncSFloat32ToInt32,
    TruncUFloat32ToInt32,
    TruncSFloat64ToInt32,
    TruncUFloat64ToInt32,
    TruncSFloat32ToInt64,
    TruncUFloat32ToInt64,
    TruncSFloat64ToInt64,
    TruncUFloat64ToInt64,
    // Conversions (Integer <-> Integer)
    WrapInt64,
    ExtendSInt32,
    ExtendUInt32,
    // Conversions (Float <-> Float)
    PromoteFloat32,
    DemoteFloat64,
    // Reinterprets
    ReinterpretFloat32,
    ReinterpretFloat64,
    ReinterpretInt32,
    ReinterpretInt64,
    // Sign Extensions (Post-MVP but standard)
    ExtendS8Int32,
    ExtendS16Int32,
    ExtendS8Int64,
    ExtendS16Int64,
    ExtendS32Int64,
}

impl UnaryOp {
    pub fn is_relational(&self) -> bool {
        matches!(self, UnaryOp::EqZInt32 | UnaryOp::EqZInt64)
    }

    pub fn may_trap(&self) -> bool {
        matches!(
            self,
            UnaryOp::TruncSFloat32ToInt32
                | UnaryOp::TruncUFloat32ToInt32
                | UnaryOp::TruncSFloat64ToInt32
                | UnaryOp::TruncUFloat64ToInt32
                | UnaryOp::TruncSFloat32ToInt64
                | UnaryOp::TruncUFloat32ToInt64
                | UnaryOp::TruncSFloat64ToInt64
                | UnaryOp::TruncUFloat64ToInt64
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum BinaryOp {
    AddInt32,
    SubInt32,
    MulInt32,
    DivSInt32,
    DivUInt32,
    RemSInt32,
    RemUInt32,
    AndInt32,
    OrInt32,
    XorInt32,
    ShlInt32,
    ShrSInt32,
    ShrUInt32,
    RotLInt32,
    RotRInt32,
    EqInt32,
    NeInt32,
    LtSInt32,
    LtUInt32,
    LeSInt32,
    LeUInt32,
    GtSInt32,
    GtUInt32,
    GeSInt32,
    GeUInt32,

    AddInt64,
    SubInt64,
    MulInt64,
    DivSInt64,
    DivUInt64,
    RemSInt64,
    RemUInt64,
    AndInt64,
    OrInt64,
    XorInt64,
    ShlInt64,
    ShrSInt64,
    ShrUInt64,
    RotLInt64,
    RotRInt64,
    EqInt64,
    NeInt64,
    LtSInt64,
    LtUInt64,
    LeSInt64,
    LeUInt64,
    GtSInt64,
    GtUInt64,
    GeSInt64,
    GeUInt64,

    AddFloat32,
    SubFloat32,
    MulFloat32,
    DivFloat32,
    CopySignFloat32,
    MinFloat32,
    MaxFloat32,
    EqFloat32,
    NeFloat32,
    LtFloat32,
    LeFloat32,
    GtFloat32,
    GeFloat32,

    AddFloat64,
    SubFloat64,
    MulFloat64,
    DivFloat64,
    CopySignFloat64,
    MinFloat64,
    MaxFloat64,
    EqFloat64,
    NeFloat64,
    LtFloat64,
    LeFloat64,
    GtFloat64,
    GeFloat64,
}

impl BinaryOp {
    pub fn is_relational(&self) -> bool {
        matches!(
            self,
            BinaryOp::EqInt32
                | BinaryOp::NeInt32
                | BinaryOp::LtSInt32
                | BinaryOp::LtUInt32
                | BinaryOp::LeSInt32
                | BinaryOp::LeUInt32
                | BinaryOp::GtSInt32
                | BinaryOp::GtUInt32
                | BinaryOp::GeSInt32
                | BinaryOp::GeUInt32
                | BinaryOp::EqInt64
                | BinaryOp::NeInt64
                | BinaryOp::LtSInt64
                | BinaryOp::LtUInt64
                | BinaryOp::LeSInt64
                | BinaryOp::LeUInt64
                | BinaryOp::GtSInt64
                | BinaryOp::GtUInt64
                | BinaryOp::GeSInt64
                | BinaryOp::GeUInt64
                | BinaryOp::EqFloat32
                | BinaryOp::NeFloat32
                | BinaryOp::LtFloat32
                | BinaryOp::LeFloat32
                | BinaryOp::GtFloat32
                | BinaryOp::GeFloat32
                | BinaryOp::EqFloat64
                | BinaryOp::NeFloat64
                | BinaryOp::LtFloat64
                | BinaryOp::LeFloat64
                | BinaryOp::GtFloat64
                | BinaryOp::GeFloat64
        )
    }

    pub fn may_trap(&self) -> bool {
        matches!(
            self,
            BinaryOp::DivSInt32
                | BinaryOp::DivUInt32
                | BinaryOp::RemSInt32
                | BinaryOp::RemUInt32
                | BinaryOp::DivSInt64
                | BinaryOp::DivUInt64
                | BinaryOp::RemSInt64
                | BinaryOp::RemUInt64
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum AtomicOp {
    Add,
    Sub,
    And,
    Or,
    Xor,
    Xchg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum SIMDOp {
    Splat,
    Extract,
    Replace,
    Shuffle,
    Ternary,
    Shift,
    Load,
    LoadStoreLane,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum SIMDExtractOp {
    ExtractLaneS8x16,
    ExtractLaneU8x16,
    ExtractLaneS16x8,
    ExtractLaneU16x8,
    ExtractLaneI32x4,
    ExtractLaneI64x2,
    ExtractLaneF32x4,
    ExtractLaneF64x2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum SIMDReplaceOp {
    ReplaceLane8x16,
    ReplaceLane16x8,
    ReplaceLane32x4,
    ReplaceLane64x2,
    ReplaceLaneF32x4,
    ReplaceLaneF64x2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum SIMDTernaryOp {
    Bitselect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum SIMDShiftOp {
    Shl8x16,
    ShrS8x16,
    ShrU8x16,
    Shl16x8,
    ShrS16x8,
    ShrU16x8,
    Shl32x4,
    ShrS32x4,
    ShrU32x4,
    Shl64x2,
    ShrS64x2,
    ShrU64x2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum SIMDLoadOp {
    Load8x8S,
    Load8x8U,
    Load16x4S,
    Load16x4U,
    Load32x2S,
    Load32x2U,
    Load8Splat,
    Load16Splat,
    Load32Splat,
    Load64Splat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum SIMDLoadStoreLaneOp {
    LoadLane8,
    LoadLane16,
    LoadLane32,
    LoadLane64,
    StoreLane8,
    StoreLane16,
    StoreLane32,
    StoreLane64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum MemoryOp {
    Init,
    Drop,
    Copy,
    Fill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RefAsOp {
    Extern,
    Func,
    Any,
    NonNull,
    I31,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RefTestOp {
    Ref,
    NotRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RefCastOp {
    Cast,
    NotCast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BrOnOp {
    Null,
    NonNull,
    Cast,
    NotCast,
}
