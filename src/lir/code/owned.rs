// audited: 2026-10-06
//! `LirCode` and `LirOwned`: a frozen function's records in `Vec`s, and the values its instructions load.
//!
//! docs/impl/lir.md
//!
//! `LirCode` is plain data: `Send`, and serializable, so it crosses a thread
//! or a file as it stands. The values a `ValueConst` loads are heap pointers
//! and are neither, so they ride beside it in `LirOwned`.

use super::record::{BlockRec, ConstRec, Node, SiteRec};
use crate::hir::region::StaticRegion;
use crate::lir::{CallSiteInfo, Reg, YieldPointInfo};
use crate::signals::Signal;
use crate::syntax::files::FileId;
use crate::syntax::Span;
use crate::value::{Arity, Value};

/// A frozen function's records, tables and header.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct LirCode {
    pub(crate) nodes: Vec<Node>,
    pub(crate) blocks: Vec<BlockRec>,
    pub(crate) pool: Vec<u32>,
    pub(crate) consts: Vec<ConstRec>,
    /// The `MaterializeConst` templates, as `ConstTemplate::encode` wrote them.
    pub(crate) data: Vec<u8>,
    /// The files the spans name, each once. A span's `file` field is one past
    /// its index here. A `FileId` is process-local, so the table serializes by
    /// spelling.
    #[serde(with = "file_names")]
    pub(crate) files: Vec<FileId>,
    /// How many values the `ValueConst` instructions index, so a `LirOwned`
    /// rebuilt from parts can check it holds them all.
    pub(crate) n_values: u32,
    pub(crate) yield_points: Vec<SiteRec>,
    pub(crate) call_sites: Vec<SiteRec>,
    pub(crate) site_regs: Vec<Reg>,
    pub(crate) closure_id: Option<u32>,
    pub(crate) name: Option<String>,
    pub(crate) arity: Arity,
    pub(crate) entry: u32,
    pub(crate) num_regs: u32,
    pub(crate) num_locals: u16,
    pub(crate) num_captures: u16,
    pub(crate) num_params: u32,
    pub(crate) num_local_params: u32,
    pub(crate) capture_params_mask: u64,
    pub(crate) capture_locals: Vec<u64>,
    pub(crate) signal: Signal,
    pub(crate) vararg_kind: crate::hir::VarargKind,
    pub(crate) rest_list_layout: crate::value::RestListLayout,
    pub(crate) region_table: Vec<StaticRegion>,
    pub(crate) merged_slots: Vec<StaticRegion>,
    pub(crate) frame_release_slots: Vec<u16>,
    pub(crate) frame_release_regions: Vec<StaticRegion>,
    /// The code payload carries the docstring and the origin, and nothing
    /// reads them off a serialized function, so neither crosses.
    #[serde(skip)]
    pub(crate) doc: Option<String>,
    #[serde(skip)]
    pub(crate) origin: Option<Span>,
}

/// A frozen function and the values its `ValueConst` instructions load.
#[derive(Clone, Debug)]
pub struct LirOwned {
    pub(crate) code: LirCode,
    pub(crate) values: Vec<Value>,
}

impl LirOwned {
    /// The read view over this function.
    pub fn view(&self) -> super::LirView<'_> {
        super::LirView::over(&self.code, &self.values)
    }

    /// The plain-data half, for a boundary that carries the values its own way.
    pub fn code(&self) -> &LirCode {
        &self.code
    }

    /// The values the `ValueConst` instructions index.
    pub fn values(&self) -> &[Value] {
        &self.values
    }

    /// A function rebuilt from its two halves. Refused when `values` is not
    /// the table `code` indexes.
    pub fn from_parts(code: LirCode, values: Vec<Value>) -> Result<LirOwned, String> {
        if values.len() != code.n_values as usize {
            return Err(format!(
                "frozen LIR indexes {} values, given {}",
                code.n_values,
                values.len()
            ));
        }
        Ok(LirOwned { code, values })
    }

    /// Record the yield points and call sites emission found. Only emission
    /// can supply them, so they arrive after the function froze.
    pub fn set_sites(&mut self, yield_points: &[YieldPointInfo], call_sites: &[CallSiteInfo]) {
        let code = &mut self.code;
        code.site_regs.clear();
        code.yield_points = yield_points
            .iter()
            .map(|y| {
                site(
                    &mut code.site_regs,
                    y.resume_ip,
                    y.num_locals,
                    &y.stack_regs,
                )
            })
            .collect();
        code.call_sites = call_sites
            .iter()
            .map(|c| {
                site(
                    &mut code.site_regs,
                    c.resume_ip,
                    c.num_locals,
                    &c.stack_regs,
                )
            })
            .collect();
    }

    /// Name a nameless function, for the JIT's code-address registry.
    pub fn set_name(&mut self, name: Option<String>) {
        self.code.name = name;
    }
}

/// One site's record, its registers appended to `regs`.
fn site(regs: &mut Vec<Reg>, resume_ip: usize, num_locals: u16, stack: &[Reg]) -> SiteRec {
    let at = regs.len() as u32;
    regs.extend_from_slice(stack);
    SiteRec {
        resume_ip: u32::try_from(resume_ip).expect("a resume address fits 32 bits"),
        num_locals,
        pad: 0,
        regs: at,
        n_regs: stack.len() as u32,
    }
}

/// A compile unit's functions, frozen: the entry function and the closures
/// its `MakeClosure` instructions name by `ClosureId`.
#[derive(Clone, Debug)]
pub struct FrozenModule {
    pub entry: LirOwned,
    pub closures: Vec<LirOwned>,
}

/// Serde for a file table: each `FileId` travels as its spelling and is
/// interned again where it lands, as a `Span`'s file is.
mod file_names {
    use crate::syntax::files::{self, FileId};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub(super) fn serialize<S: Serializer>(ids: &[FileId], ser: S) -> Result<S::Ok, S::Error> {
        let names: Vec<&str> = ids
            .iter()
            .map(|id| files::name(*id).unwrap_or(""))
            .collect();
        names.serialize(ser)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<Vec<FileId>, D::Error> {
        let names: Vec<String> = Vec::deserialize(de)?;
        Ok(names.iter().map(|n| files::intern(n)).collect())
    }
}
