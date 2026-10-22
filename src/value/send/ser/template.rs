// audited: 2026-10-21
//! Serializing a code object into a `SendableClosure`: its fields, the values and code objects it names, and its spellings.
//!
//! docs/threads.md
//! docs/impl/region/template.md
//!
//! Both code paths build their `SendableClosure` here. A closure instance
//! interns into the bundle and brings its `env` and squelch mask; a code
//! object a `MakeClosure` indexes has no heap identity to intern, so it is a
//! straight recursive copy with both empty.

use super::super::*;
use super::ctx::SerContext;
use super::from_value_inner;
use crate::value::closure::ClosureTemplate;

/// A code object's frozen LIR as it crosses: the records verbatim, and the
/// values its `ValueConst`s load through the ordinary value walk, so a closure
/// among them interns into the bundle once.
fn send_lir(
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
fn send_children(
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
    sendable(t, Vec::new(), SignalBits::EMPTY, ctx)
}

/// `t` as a `SendableClosure` carrying `env` and `squelch_mask`: its constants,
/// LIR and children through the walks above, every other field copied, and
/// the spelling of each name its payload holds outside its values noted.
pub(in crate::value::send) fn sendable(
    t: &ClosureTemplate,
    env: Vec<SendValue>,
    squelch_mask: SignalBits,
    ctx: &mut SerContext<'_>,
) -> Result<SendableClosure, String> {
    ctx.note_payload_names(t);
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
        ..code_fields(t, env, squelch_mask)
    })
}

/// Every scalar and table field of `t`, for a `SendableClosure` carrying `env`
/// and `squelch_mask`. The constants, the LIR and the children walk the value
/// graph, so [`sendable`] fills them; a closure's intern placeholder leaves
/// them empty.
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
        fragment: t.fragment().to_vec(),
    }
}
