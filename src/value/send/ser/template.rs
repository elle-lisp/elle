// audited: 2026-10-06
// docs/threads.md
// docs/impl/region/template.md
//! Serializing a code object a `MakeClosure` indexes into a `SendableClosure`.
//!
//! Kept apart from the instance path in `from_value_inner`: such a code object
//! has no heap identity to intern, so its serialization is a straight
//! recursive copy with empty `env`/`squelch_mask`, distinct enough to read on
//! its own.

use super::super::*;
use super::ctx::SerContext;
use super::from_value_inner;
use crate::value::closure::ClosureTemplate;

/// A code object's frozen LIR as it crosses: the records verbatim, and the
/// values its `ValueConst`s load through the ordinary value walk, so a closure
/// among them interns into the bundle once.
pub(in crate::value::send) fn send_lir(
    lir: Option<crate::lir::LirView<'_>>,
    ctx: &mut SerContext<'_>,
) -> Result<(Option<crate::lir::LirCode>, Vec<SendValue>), String> {
    let Some(lir) = lir else {
        return Ok((None, Vec::new()));
    };
    let values = lir
        .values()
        .iter()
        .map(|v| from_value_inner(*v, ctx))
        .collect::<Result<_, _>>()?;
    Ok((Some(lir.to_owned().code), values))
}

/// The code objects `t`'s `MakeClosure` instructions index, each serialized
/// whole, in child-table order.
pub(in crate::value::send) fn send_children(
    t: &ClosureTemplate,
    ctx: &mut SerContext<'_>,
) -> Result<Vec<SendableClosure>, String> {
    (0..t.num_children())
        .map(|i| sendable_from_template(&t.child(i), ctx))
        .collect()
}

/// Serialize a code object into a `SendableClosure`. A code object has no heap
/// identity to intern, so it is emitted inline; `env`/`squelch_mask` are empty.
/// Recurses on the child table, so a receiver rebuilds the full nested-lambda
/// tree and every `MakeClosure` resolves.
pub(in crate::value::send) fn sendable_from_template(
    t: &ClosureTemplate,
    ctx: &mut SerContext<'_>,
) -> Result<SendableClosure, String> {
    let constants: Vec<SendValue> = t
        .constants()
        .iter()
        .map(|v| from_value_inner(*v, ctx))
        .collect::<Result<_, _>>()?;
    let (lir, lir_values) = send_lir(t.lir(), ctx)?;
    let child_protos = send_children(t, ctx)?;
    Ok(SendableClosure {
        constants,
        lir,
        lir_values,
        child_protos,
        ..code_fields(t, Vec::new(), SignalBits::EMPTY)
    })
}

/// Every scalar and table field of `t`, for a `SendableClosure` carrying `env`
/// and `squelch_mask`. The constants, the LIR and the children walk the value
/// graph, so the caller fills them.
pub(in crate::value::send) fn code_fields(
    t: &ClosureTemplate,
    env: Vec<SendValue>,
    squelch_mask: SignalBits,
) -> SendableClosure {
    SendableClosure {
        bytecode: t.bytecode().to_vec(),
        arity: t.arity(),
        num_locals: t.num_locals(),
        num_captures: t.num_captures(),
        num_params: t.num_params(),
        constants: Vec::new(),
        signal: t.signal(),
        capture_params_mask: t.capture_params_mask(),
        capture_locals_mask: t.owned_capture_locals_mask(),
        location_map: t.location_map(),
        doc: t.doc().map(str::to_string),
        vararg_kind: t.vararg_kind(),
        rest_list_layout: t.rest_list_layout(),
        name: t.name().map(str::to_string),
        squelch_mask,
        env,
        lir: None,
        lir_values: Vec::new(),
        child_protos: Vec::new(),
        merged_slots: t.merged_slots().as_slice().to_vec(),
        frame_release_slots: t.frame_release_slots().to_vec(),
        frame_release_regions: t.frame_release_regions().to_vec(),
    }
}
