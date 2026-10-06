// audited: 2026-10-06
//! What a view answers about a function as a whole, built from either home: a `LirCode`, or a body beside its payload.
//!
//! docs/impl/lir.md
//!
//! A `LirCode` holds every header field itself. A `LirBody` holds only what
//! LIR alone knows, and the payload it sits in holds the rest, so a view over
//! a body takes those fields from the payload as `PayloadHeader`. Either way
//! the view answers through one `Head`, and no reader learns which home it
//! reads.

use super::body::LirBody;
use super::owned::LirCode;
use crate::hir::region::StaticRegion;
use crate::hir::VarargKind;
use crate::signals::Signal;
use crate::syntax::files::{self, FileId};
use crate::syntax::Span;
use crate::value::closure::{RestListLayout, StrKeys, VarargTag};
use crate::value::region_slice::RegionSlice;
use crate::value::Arity;

/// The header fields a code payload holds for the body inside it, which a
/// view over that body reads there (docs/impl/lir.md).
#[derive(Clone, Copy, Debug)]
pub(crate) struct PayloadHeader<'a> {
    pub(crate) name: Option<&'a str>,
    pub(crate) doc: Option<&'a str>,
    pub(crate) origin: Option<Span>,
    pub(crate) arity: Arity,
    pub(crate) signal: Signal,
    pub(crate) num_locals: u16,
    pub(crate) num_params: u32,
    pub(crate) capture_params_mask: u64,
    pub(crate) capture_locals: &'a [u64],
    pub(crate) vararg: VarargTag,
    pub(crate) strict_keys: StrKeys<'a>,
    pub(crate) rest_list_layout: RestListLayout,
    pub(crate) region_table: &'a [StaticRegion],
}

/// How a function's rest parameter collects, in whichever form its home
/// keeps: the compile-time kind, or a payload's tag and key set.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Vararg<'a> {
    Kind(&'a VarargKind),
    Tagged(VarargTag, StrKeys<'a>),
}

impl Vararg<'_> {
    /// The compile-time kind. Allocates for a `&named` key set alone.
    pub(crate) fn kind(&self) -> VarargKind {
        match *self {
            Vararg::Kind(kind) => kind.clone(),
            Vararg::Tagged(VarargTag::List, _) => VarargKind::List,
            Vararg::Tagged(VarargTag::Struct, _) => VarargKind::Struct,
            Vararg::Tagged(VarargTag::StrictStruct, keys) => {
                VarargKind::StrictStruct(keys.iter().map(str::to_string).collect())
            }
        }
    }
}

/// The files a function's spans name, in whichever form its home keeps.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Files<'a> {
    /// A `LirCode`'s table: this process's ids.
    Ids(&'a [FileId]),
    /// A body's table: spellings, interned again where a span is read.
    Names(&'a [RegionSlice<u8>]),
}

impl<'a> Files<'a> {
    /// The file at index `ix`, as this process names it.
    pub(crate) fn id(&self, ix: usize) -> FileId {
        match *self {
            Files::Ids(ids) => ids[ix],
            Files::Names(names) => files::intern(spelling(&names[ix])),
        }
    }

    /// Every file's spelling, in table order.
    pub(crate) fn names(&self) -> Vec<&'a str> {
        match *self {
            Files::Ids(ids) => ids
                .iter()
                .map(|id| files::name(*id).unwrap_or(""))
                .collect(),
            Files::Names(names) => names.iter().map(spelling).collect(),
        }
    }

    /// Every file's id in this process, in table order.
    pub(crate) fn ids(&self) -> Vec<FileId> {
        match *self {
            Files::Ids(ids) => ids.to_vec(),
            Files::Names(names) => names.iter().map(|n| files::intern(spelling(n))).collect(),
        }
    }
}

/// A body's file spelling as `str`. The bytes came from a `str` when the
/// body was built, so they are UTF-8 by construction.
fn spelling(name: &RegionSlice<u8>) -> &str {
    std::str::from_utf8(name.as_slice()).expect("an LIR file spelling is UTF-8 by construction")
}

/// Everything a view answers about the function as a whole.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Head<'a> {
    pub(crate) closure_id: Option<u32>,
    pub(crate) name: Option<&'a str>,
    pub(crate) doc: Option<&'a str>,
    pub(crate) origin: Option<Span>,
    pub(crate) arity: Arity,
    pub(crate) entry: u32,
    pub(crate) num_regs: u32,
    pub(crate) num_locals: u16,
    pub(crate) num_captures: u16,
    pub(crate) num_params: u32,
    pub(crate) num_local_params: u32,
    pub(crate) capture_params_mask: u64,
    pub(crate) capture_locals: &'a [u64],
    pub(crate) signal: Signal,
    pub(crate) vararg: Vararg<'a>,
    pub(crate) rest_list_layout: RestListLayout,
    pub(crate) region_table: &'a [StaticRegion],
    pub(crate) merged_slots: &'a [StaticRegion],
    pub(crate) frame_release_slots: &'a [u16],
    pub(crate) frame_release_regions: &'a [StaticRegion],
}

impl<'a> Head<'a> {
    pub(crate) fn of_code(code: &'a LirCode) -> Head<'a> {
        Head {
            closure_id: code.closure_id,
            name: code.name.as_deref(),
            doc: code.doc.as_deref(),
            origin: code.origin,
            arity: code.arity,
            entry: code.entry,
            num_regs: code.num_regs,
            num_locals: code.num_locals,
            num_captures: code.num_captures,
            num_params: code.num_params,
            num_local_params: code.num_local_params,
            capture_params_mask: code.capture_params_mask,
            capture_locals: &code.capture_locals,
            signal: code.signal,
            vararg: Vararg::Kind(&code.vararg_kind),
            rest_list_layout: code.rest_list_layout,
            region_table: &code.region_table,
            merged_slots: &code.merged_slots,
            frame_release_slots: &code.frame_release_slots,
            frame_release_regions: &code.frame_release_regions,
        }
    }

    pub(crate) fn of_body(body: &'a LirBody, payload: PayloadHeader<'a>) -> Head<'a> {
        Head {
            closure_id: body.has_closure_id.then_some(body.closure_id),
            name: payload.name,
            doc: payload.doc,
            origin: payload.origin,
            arity: payload.arity,
            entry: body.entry,
            num_regs: body.num_regs,
            num_locals: payload.num_locals,
            num_captures: body.num_captures,
            num_params: payload.num_params,
            num_local_params: body.num_local_params,
            capture_params_mask: payload.capture_params_mask,
            capture_locals: payload.capture_locals,
            signal: payload.signal,
            vararg: Vararg::Tagged(payload.vararg, payload.strict_keys),
            rest_list_layout: payload.rest_list_layout,
            region_table: payload.region_table,
            merged_slots: body.merged_slots.as_slice(),
            frame_release_slots: body.frame_release_slots.as_slice(),
            frame_release_regions: body.frame_release_regions.as_slice(),
        }
    }
}
