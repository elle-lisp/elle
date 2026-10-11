// audited: 2026-10-06
//! Freezing a finished function: its blocks' nodes into one table, its tables into exact-size `Vec`s, its header across.
//!
//! docs/impl/lir.md

use super::encode;
use super::Work;
use crate::lir::code::{LirCode, LirOwned};

/// The frozen form of `work`, which leaves its header taken.
///
/// Refused when an instruction could not be encoded, which `emit` records
/// rather than reports, so the lowerer sees the error where the function ends.
pub(super) fn freeze(work: &mut Work) -> Result<LirOwned, String> {
    let Work {
        head,
        blocks: built,
        tables,
        ..
    } = work;
    if let Some(why) = tables.refused.take() {
        return Err(format!("freeze: {why}"));
    }
    let total = built.iter().map(|b| b.nodes.len()).sum();
    let mut nodes = Vec::with_capacity(total);
    let mut blocks = Vec::with_capacity(built.len());
    for b in built.iter() {
        let mut rec = encode::exit(tables, b.label, b.exit, b.span);
        rec.first = nodes.len() as u32;
        rec.len = b.nodes.len() as u32;
        nodes.extend_from_slice(b.nodes.as_slice());
        blocks.push(rec);
    }
    let code = LirCode {
        nodes,
        blocks,
        pool: tables.pool.as_slice().to_vec(),
        consts: tables.consts.as_slice().to_vec(),
        data: tables.data.as_slice().to_vec(),
        n_values: tables.values.len() as u32,
        files: tables.files.as_slice().to_vec(),
        yield_points: Vec::new(),
        call_sites: Vec::new(),
        site_regs: Vec::new(),
        closure_id: head.closure_id.map(|c| c.0),
        name: head.name.take(),
        arity: head.arity,
        entry: head.entry.0,
        num_regs: head.num_regs,
        num_locals: head.num_locals,
        num_captures: head.num_captures,
        num_params: head.num_params as u32,
        num_local_params: head.num_local_params as u32,
        capture_params_mask: head.capture_params_mask,
        capture_locals: head.capture_locals_mask.words().to_vec(),
        signal: head.signal,
        vararg_kind: head.vararg_kind.clone(),
        rest_list_layout: head.rest_list_layout,
        region_table: std::mem::take(&mut head.region_table),
        merged_slots: ascending(&mut head.merged_slots, |s| s.get()),
        frame_release_slots: ascending(&mut head.frame_release_slots, |s| *s),
        frame_release_regions: ascending(&mut head.frame_release_regions, |r| r.get()),
        doc: head.doc.as_deref().map(str::to_string),
        origin: head.origin,
    };
    Ok(LirOwned {
        code,
        values: tables.values.as_slice().to_vec(),
    })
}

/// `table`, taken and sorted ascending by `key`. The merge set and the release
/// tables are recorded this way, so the payload a function's code object holds
/// and every view over the frozen function agree on order as well as content
/// (docs/impl/lir.md).
fn ascending<T: Copy, K: Ord>(table: &mut Vec<T>, key: impl Fn(&T) -> K) -> Vec<T> {
    let mut sorted = std::mem::take(table);
    sorted.sort_unstable_by_key(key);
    sorted
}
