// audited: 2026-10-06
//! `LirHead`: the header of the function under construction, written by the lowerer beside its instructions.
//!
//! docs/impl/lir.md

use crate::hir::region::StaticRegion;
use crate::hir::VarargKind;
use crate::lir::{ClosureId, Label};
use crate::signals::Signal;
use crate::syntax::Span;
use crate::value::{Arity, CaptureMask, RestListLayout};

/// What a function carries beside its blocks. Every reader of the frozen
/// function answers these through its `LirView`.
#[derive(Debug, Clone)]
pub struct LirHead {
    /// This closure's place in the unit's closure list; `None` for the entry
    /// function.
    pub closure_id: Option<ClosureId>,
    /// The name a binder gave the lambda, for `fn/signature` and display.
    pub name: Option<String>,
    pub arity: Arity,
    pub entry: Label,
    pub num_regs: u32,
    pub num_locals: u16,
    /// Captured variables, which the JIT reads from the environment rather
    /// than from the arguments.
    pub num_captures: u16,
    /// Bit `i` is set when parameter `i` needs a capture cell.
    pub capture_params_mask: u64,
    /// The locally-defined variables that need capture cells, indexed from the
    /// first local after the parameters. Unbounded, so an uncaptured high local
    /// is never celled.
    pub capture_locals_mask: CaptureMask,
    pub signal: Signal,
    /// The source lambda's docstring: compile-time data, materialized as a
    /// fresh value only when `(doc f)` asks.
    pub doc: Option<std::rc::Rc<str>>,
    /// Where the lambda was written, for `(meta/origin f)`.
    pub origin: Option<Span>,
    /// How the rest parameter collects; meaningful only for `Arity::AtLeast`.
    pub vararg_kind: VarargKind,
    /// How a `&` rest list is built: the region analysis' verdict on this
    /// lambda (docs/impl/region/restlist.md), read by the JIT prologue and
    /// written into the code payload.
    pub rest_list_layout: RestListLayout,
    /// Every parameter slot: required, optional, and the rest if present.
    pub num_params: usize,
    /// The parameters copied to local slots, which come first among the
    /// locals.
    pub num_local_params: usize,
    /// The static region slots the lowerer minted for this function.
    pub region_table: Vec<StaticRegion>,
    /// The slots two or more allocations share after a builder-idiom merge
    /// (docs/impl/region/merging.md).
    pub merged_slots: Vec<StaticRegion>,
    /// The local slots the value-routed releases read, for the abandoned-frame
    /// walk (docs/impl/region/unwind.md).
    pub frame_release_slots: Vec<u16>,
    /// The static slots the slot-routed releases name, the other half of the
    /// same table.
    pub frame_release_regions: Vec<StaticRegion>,
}

impl LirHead {
    /// The header of a function of `arity` with every other field at its
    /// default: silent, no captures, no locals, entry at label 0.
    pub fn new(arity: Arity) -> LirHead {
        LirHead {
            closure_id: None,
            name: None,
            arity,
            entry: Label(0),
            num_regs: 0,
            num_locals: 0,
            num_captures: 0,
            capture_params_mask: 0,
            capture_locals_mask: CaptureMask::empty(),
            signal: Signal::silent(),
            doc: None,
            origin: None,
            vararg_kind: VarargKind::List,
            rest_list_layout: RestListLayout::PerCell,
            num_params: arity.fixed_params(),
            num_local_params: 0,
            region_table: Vec::new(),
            merged_slots: Vec::new(),
            frame_release_slots: Vec::new(),
            frame_release_regions: Vec::new(),
        }
    }
}
