// audited: 2026-09-14
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
use super::lir::convert_lir_for_send;
use crate::value::closure::{ChildCode, ClosureTemplate};

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
/// cross, so every field comes off the payload and the LIR is absent — the
/// worker runs it on the interpreter tier, exactly as this process does.
fn sendable_from_header(
    t: &ClosureTemplate,
    ctx: &mut SerContext<'_>,
) -> Result<SendableClosure, String> {
    let constants: Vec<SendValue> = t
        .constants()
        .iter()
        .map(|v| from_value_inner(*v, ctx))
        .collect::<Result<_, _>>()?;

    let child_protos: Vec<SendableClosure> = (0..t.num_children())
        .map(|i| sendable_from_child(t.child(i), ctx))
        .collect::<Result<_, _>>()?;

    Ok(SendableClosure {
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
        name: t.name().map(str::to_string),
        squelch_mask: SignalBits::EMPTY,
        env: Vec::new(),
        lir_function: None,
        lir_value_pool: Vec::new(),
        child_protos,
        merged_slots: t.merged_slots().as_slice().to_vec(),
        frame_release_slots: t.frame_release_slots().to_vec(),
        frame_release_regions: t.frame_release_regions().to_vec(),
    })
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

    let (lir_function, lir_value_pool) = match t.lir_function.as_ref() {
        Some(lir) => {
            let mut lir = (**lir).clone();
            lir.doc = None;
            match convert_lir_for_send(&mut lir, ctx)? {
                Some(pool) => (Some(lir), pool),
                None => (None, Vec::new()),
            }
        }
        None => (None, Vec::new()),
    };

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
        name: t.name.clone(),
        squelch_mask: SignalBits::EMPTY,
        env: Vec::new(),
        lir_function,
        lir_value_pool,
        child_protos,
        merged_slots: t.merged_slots.iter().copied().collect(),
        frame_release_slots: t.frame_release_slots.clone(),
        frame_release_regions: t.frame_release_regions.clone(),
    })
}
