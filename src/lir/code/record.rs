// audited: 2026-10-06
//! The plain records a frozen function is made of: nodes, blocks, constants and sites.
//!
//! docs/impl/lir.md
//!
//! Every record is `repr(C)` with no implicit padding — a pad is a named field
//! that freezing writes as zero — so a copy of a record's bytes is a copy of
//! its meaning and nothing else.

use crate::lir::Reg;

/// The register an instruction with no destination stores, and the unused
/// half of a node's inline uses. `Reg` ids are dense from zero, so the top
/// value is free.
pub const NO_REG: u32 = u32::MAX;

/// A span's file field when the span names no file. File indices count from
/// one into the function's file table.
pub const NO_FILE: u32 = 0;

/// One instruction: 48 bytes, every one of them written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(C)]
pub struct Node {
    pub(crate) start: u32,
    pub(crate) end: u32,
    pub(crate) line: u32,
    pub(crate) col: u32,
    /// One past the index of the span's file in the function's file table, or
    /// `NO_FILE`.
    pub(crate) file: u32,
    /// The variant's `dst` field, or `NO_REG` for a variant with none.
    pub(crate) dst: u32,
    /// What `LirInstr::region` answers, or zero for a variant with no region.
    pub(crate) region: u32,
    /// The variant's one scalar beside its registers: a slot, a capture
    /// index, a closure id, a region slot, or an index into a function table.
    pub(crate) aux: u32,
    /// The first two register uses. A node with more holds every use in the
    /// pool, from `extra`, and these two again.
    pub(crate) uses: [Reg; 2],
    /// Where this node's run starts in the function's pool: its uses when
    /// there are more than two, then the scalars the variant carries.
    pub(crate) extra: u32,
    pub(crate) n_uses: u16,
    pub(crate) op: u8,
    /// A sub-operation selector in the low four bits, and the variant's
    /// booleans above them.
    pub(crate) flags: u8,
}

/// One basic block: its label, its run of nodes, and its terminator.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(C)]
pub struct BlockRec {
    pub(crate) label: u32,
    /// The block's first node, an index into the function's node table.
    pub(crate) first: u32,
    pub(crate) len: u32,
    /// The terminator's operands; `term_op` says what each one is.
    pub(crate) term_a: u32,
    pub(crate) term_b: u32,
    pub(crate) term_c: u32,
    pub(crate) start: u32,
    pub(crate) end: u32,
    pub(crate) line: u32,
    pub(crate) col: u32,
    pub(crate) file: u32,
    pub(crate) term_op: u8,
    pub(crate) pad: [u8; 3],
}

/// An immediate constant: a kind byte and 64 bits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(C)]
pub struct ConstRec {
    pub(crate) kind: u8,
    pub(crate) pad: [u8; 7],
    pub(crate) bits: u64,
}

impl ConstRec {
    /// The record an immediate constant is stored as, for an instruction being
    /// built: a `StructRest`'s keys are a run of these.
    pub const fn immediate(c: super::operand::ConstRef) -> ConstRec {
        use super::operand::ConstRef as C;
        let (kind, bits) = match c {
            C::Nil => (kind::NIL, 0),
            C::EmptyList => (kind::EMPTY_LIST, 0),
            C::Bool(b) => (kind::BOOL, b as u64),
            C::Int(n) => (kind::INT, n as u64),
            C::Float(f) => (kind::FLOAT, f.to_bits()),
            C::Symbol(s) => (kind::SYMBOL, s.0),
            C::Keyword(k) => (kind::KEYWORD, k),
        };
        ConstRec {
            kind,
            pad: [0; 7],
            bits,
        }
    }
}

/// A yield point or a call site: where the interpreter resumes, and the
/// registers live on the operand stack there.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(C)]
pub struct SiteRec {
    pub(crate) resume_ip: u32,
    pub(crate) num_locals: u16,
    pub(crate) pad: u16,
    /// The site's run in the function's site-register table.
    pub(crate) regs: u32,
    pub(crate) n_regs: u32,
}

/// `BlockRec::term_op`: how a block exits.
pub(crate) mod term {
    pub(crate) const RETURN: u8 = 0;
    pub(crate) const JUMP: u8 = 1;
    pub(crate) const BRANCH: u8 = 2;
    pub(crate) const EMIT: u8 = 3;
    pub(crate) const UNREACHABLE: u8 = 4;
}

/// `ConstRec::kind`: what the 64 bits hold.
pub(crate) mod kind {
    pub(crate) const NIL: u8 = 0;
    pub(crate) const EMPTY_LIST: u8 = 1;
    pub(crate) const BOOL: u8 = 2;
    pub(crate) const INT: u8 = 3;
    pub(crate) const FLOAT: u8 = 4;
    pub(crate) const SYMBOL: u8 = 5;
    pub(crate) const KEYWORD: u8 = 6;
    /// Raw bits a variant carries: a signal mask, a symbol id.
    pub(crate) const BITS: u8 = 7;
}
