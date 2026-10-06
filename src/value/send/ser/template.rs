// audited: 2026-10-06
// docs/threads.md
// docs/impl/image/sealing.md
//! Serializing a code object a `MakeClosure` indexes into a `SendableClosure`.
//!
//! Kept apart from the instance path in `from_value_inner`: such a code object
//! has no heap identity to intern, so its serialization is a straight
//! recursive copy with empty `env`/`squelch_mask`, distinct enough to read on
//! its own. A worker rebuilds a blueprint out of whichever side answered, so
//! its own `MakeClosure` resolves by index either way
//! (docs/impl/image/sealing.md).

use super::super::*;
use super::ctx::SerContext;
use super::from_value_inner;
use crate::value::closure::{ChildCode, ClosureTemplate};

/// A code object's frozen LIR as it crosses: the records verbatim, and the
/// values its `ValueConst`s load through the ordinary value walk, so a closure
/// among them interns into the bundle once. Every side a code object can have
/// — a blueprint, a materialized payload, an image's — crosses through here.
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

/// Serialize one child code object, from whichever side its parent answered.
pub(in crate::value::send) fn sendable_from_child(
    child: ChildCode<'_>,
    ctx: &mut SerContext<'_>,
) -> Result<SendableClosure, String> {
    match child {
        ChildCode::Blueprint(proto) => sendable_from_template(proto, ctx),
        ChildCode::Header(header) => sendable_from_header(&header, ctx),
    }
}

/// Serialize a child that came out of an image's body. Its blueprint did not
/// cross, so every field comes off the payload — the LIR included, so the
/// worker's JIT can promote it as this process's can.
fn sendable_from_header(
    t: &ClosureTemplate,
    ctx: &mut SerContext<'_>,
) -> Result<SendableClosure, String> {
    let constants: Vec<SendValue> = t
        .constants()
        .iter()
        .map(|v| from_value_inner(*v, ctx))
        .collect::<Result<_, _>>()?;

    let (lir, lir_values) = send_lir(t.lir(), ctx)?;

    let child_protos: Vec<SendableClosure> = (0..t.num_children())
        .map(|i| sendable_from_child(t.child_code(i), ctx))
        .collect::<Result<_, _>>()?;

    Ok(SendableClosure {
        lir,
        lir_values,
        ..sendable_header(t, constants, child_protos)
    })
}

/// Every field a code object's header answers, around the constants and the
/// children the caller serialized through its own context. The instance fields
/// and the LIR are empty; a caller that has them sets them over this.
pub(in crate::value::send) fn sendable_header(
    t: &ClosureTemplate,
    constants: Vec<SendValue>,
    child_protos: Vec<SendableClosure>,
) -> SendableClosure {
    SendableClosure {
        bytecode: t.bytecode().to_vec(),
        arity: t.arity(),
        num_locals: t.num_locals(),
        num_captures: t.num_captures(),
        num_params: t.num_params(),
        constants,
        signal: t.signal(),
        capture_params_mask: t.capture_params_mask(),
        capture_locals_mask: t.owned_capture_locals_mask(),
        location_map: t.location_map(),
        doc: t.doc().map(str::to_string),
        vararg_kind: t.vararg_kind(),
        rest_list_layout: t.rest_list_layout(),
        name: t.name().map(str::to_string),
        squelch_mask: SignalBits::EMPTY,
        env: Vec::new(),
        lir: None,
        lir_values: Vec::new(),
        child_protos,
        merged_slots: t.merged_slots().as_slice().to_vec(),
        frame_release_slots: t.frame_release_slots().to_vec(),
        frame_release_regions: t.frame_release_regions().to_vec(),
    }
}

/// Serialize a nested-lambda blueprint into a `SendableClosure`. A blueprint
/// has no heap identity to intern, so it is emitted inline; `env`/
/// `squelch_mask` are empty. Recurses on the blueprint's own `child_protos` so
/// a worker rebuilds the full nested-lambda tree and every `MakeClosure`
/// resolves.
pub(in crate::value::send) fn sendable_from_template(
    t: &crate::value::TemplateProto,
    ctx: &mut SerContext<'_>,
) -> Result<SendableClosure, String> {
    let constants: Vec<SendValue> = t
        .constants
        .iter()
        .map(|v| from_value_inner(*v, ctx))
        .collect::<Result<_, _>>()?;

    let doc = t.doc.clone();

    let (lir, lir_values) = send_lir(t.lir_function.as_ref().map(|l| l.view()), ctx)?;

    let child_protos: Vec<SendableClosure> = t
        .child_protos
        .iter()
        .map(|p| sendable_from_template(p, ctx))
        .collect::<Result<_, _>>()?;

    Ok(SendableClosure {
        bytecode: t.bytecode.clone(),
        arity: t.arity,
        num_locals: t.num_locals,
        num_captures: t.num_captures,
        num_params: t.num_params,
        constants,
        signal: t.signal,
        capture_params_mask: t.capture_params_mask,
        capture_locals_mask: t.capture_locals_mask.clone(),
        location_map: t.location_map.clone(),
        doc,
        vararg_kind: t.vararg_kind.clone(),
        rest_list_layout: t.rest_list_layout,
        name: t.name.clone(),
        squelch_mask: SignalBits::EMPTY,
        env: Vec::new(),
        lir,
        lir_values,
        child_protos,
        merged_slots: t.merged_slots.iter().copied().collect(),
        frame_release_slots: t.frame_release_slots.clone(),
        frame_release_regions: t.frame_release_regions.clone(),
    })
}
