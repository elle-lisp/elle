// audited: 2026-09-21
//! The region-native LIR prototype: a fixed-size POD instruction in region
//! pages.
//!
//! docs/impl/image/measurements.md
//!
//! The node drops no information the Rust-heap form carries. Registers come
//! from `for_each_def`/`for_each_use`, the region slot from `LirInstr::region`,
//! and the constants from the function's own pool; everything variable-length
//! moves to one operand pool per function, named by an offset. What is left is
//! 48 bytes, `Copy`, with no `Drop` and no pointer to the Rust heap.

// Every field below is written by the encoder, because the point of the
// prototype is that it drops nothing. The measured reads touch the fields a
// backend touches, which is fewer of them.
#![allow(dead_code)]

use elle::syntax::Span;
use elle::value::region_slice::{RegionSlice, RegionStr};
use elle::value::Value;

/// No register: `Reg` is a dense `u32`, so the top value is free.
pub const NO_REG: u32 = u32::MAX;

/// One LIR instruction, 48 bytes of plain data.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct PInstr {
    pub span: Span,
    pub dst: u32,
    /// The static region slot, 0 where the variant carries none.
    pub region: u32,
    /// A local slot, a capture index, a closure id, or a constant-pool index.
    pub aux: u32,
    pub use0: u32,
    pub use1: u32,
    /// Where this instruction's block starts in the function's operand pool.
    /// The block holds the uses past the first two, then the side scalars the
    /// variant carries.
    pub extra: u32,
    pub n_uses: u16,
    pub op: u8,
    /// The sub-operation (a `BinOp`, `CmpOp`, `UnaryOp` or `ConvOp` selector)
    /// in the low four bits, and the variant's booleans above them.
    pub flags: u8,
}

/// One basic block: its terminator inline, its instructions one slice.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct PBlock {
    pub instrs: RegionSlice<PInstr>,
    pub span: Span,
    pub label: u32,
    pub term_a: u32,
    pub term_b: u32,
    pub term_c: u32,
    pub term_op: u8,
}

/// A constant, flattened out of `LirConst` and the `ValueConst` operand.
#[derive(Clone, Copy)]
pub enum PConst {
    Nil,
    EmptyList,
    Bool(bool),
    Int(i64),
    Float(f64),
    Symbol(u64),
    Keyword(u64),
    Str(RegionStr),
    Val(Value),
    ClosureRef(u32),
    ValueRef(u32),
    /// The root of a flattened `ConstTemplate`, indexing `PFunc::templates`.
    Template(u32),
    /// A raw scalar a variant carries: a signal mask, a symbol id.
    Bits(u64),
}

/// One node of a flattened `ConstTemplate` tree. Children are indices into the
/// same array, listed in the operand pool.
#[derive(Clone, Copy)]
pub struct PTemplate {
    pub kind: u8,
    pub flag: bool,
    pub bits: u64,
    pub text: RegionStr,
    pub kids: u32,
    pub n_kids: u32,
    pub span: Span,
}

/// A yield point or a call site: its resume address and its live stack.
#[derive(Clone, Copy)]
pub struct PSite {
    pub resume_ip: u32,
    pub num_locals: u16,
    pub regs: u32,
    pub n_regs: u32,
}

/// One function. Every field is a slice into the one region this function was
/// built in, so the whole function is contiguous and frees with the region.
#[derive(Clone, Copy)]
pub struct PFunc {
    pub blocks: RegionSlice<PBlock>,
    pub consts: RegionSlice<PConst>,
    pub pool: RegionSlice<u32>,
    pub templates: RegionSlice<PTemplate>,
    pub capture_locals: RegionSlice<u64>,
    pub region_table: RegionSlice<u32>,
    pub merged_slots: RegionSlice<u32>,
    pub frame_release_slots: RegionSlice<u16>,
    pub frame_release_regions: RegionSlice<u32>,
    pub yield_points: RegionSlice<PSite>,
    pub call_sites: RegionSlice<PSite>,
    pub name: RegionStr,
    pub span: Span,
    pub closure_id: u32,
    pub entry: u32,
    pub num_regs: u32,
    pub num_params: u32,
    pub num_local_params: u32,
    pub arity_kind: u8,
    pub arity_n: u32,
    pub vararg_kind: u8,
    pub num_locals: u16,
    pub num_captures: u16,
    pub capture_params_mask: u64,
    pub signal_bits: u64,
    pub signal_propagates: u32,
}
