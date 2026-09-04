//! Serializing a code-object *blueprint* into a `SendableClosure`.
//!
//! Kept apart from the instance path in `from_value_inner`: a blueprint has no
//! heap identity to intern, so its serialization is a straight recursive copy
//! with empty `env`/`squelch_mask`, distinct enough to read on its own.

use super::super::*;
use super::ctx::SerContext;
use super::from_value_inner;
use super::lir::convert_lir_for_send;

/// Serialize a nested-lambda blueprint (a `child_protos` entry) into a
/// `SendableClosure`. A blueprint has no heap identity to intern, so it is
/// emitted inline; `env`/`squelch_mask` are empty. Recurses on the blueprint's
/// own `child_protos` so a worker rebuilds the full nested-lambda tree and
/// every `MakeClosure` resolves.
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
