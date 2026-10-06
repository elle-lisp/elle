// audited: 2026-10-06
// docs/threads.md
//! Serializing a live closure instance into the bundle's intern table.
//!
//! Split from the value-tag `match` because the closure arm manages cycle
//! detection: it pre-inserts a placeholder before recursing into `env`, the
//! constants and the LIR, and it carries the full nested-lambda tree.
//! Isolating it keeps the tag dispatch readable.

use super::super::*;
use super::ctx::SerContext;
use super::from_value_inner;
use super::template::{code_fields, send_children, send_lir};

/// Serialize a closure instance reached at heap value `value`, interning it
/// into `ctx.closures` with cycle detection and returning a `Ref` to its slot.
///
/// `closure_rc` is the derefed closure payload. `value.payload` is the identity
/// key: for heap values, payload IS the pointer, so it uniquely names the
/// closure across the (possibly cyclic) graph.
pub(super) fn send_closure(
    value: Value,
    closure_rc: &crate::value::closure::Closure,
    ctx: &mut SerContext<'_>,
) -> Result<SendValue, String> {
    // Use value.payload as identity key — for heap values, payload IS the pointer.
    let key = value.payload;

    // Already visited → return Ref to existing intern entry.
    if let Some(&idx) = ctx.visited.get(&key) {
        return Ok(SendValue::Ref(idx));
    }

    // Reserve an index BEFORE recursing so back-references resolve to this
    // entry. The placeholder is overwritten below.
    let template = &closure_rc.template;
    let idx = ctx.closures.len();
    ctx.closures
        .push(code_fields(template, Vec::new(), SignalBits::EMPTY));
    ctx.visited.insert(key, idx);

    // Serialize environment (may contain back-references to this closure via LBox).
    let env = closure_rc
        .env
        .iter()
        .map(|v| from_value_inner(*v, ctx))
        .collect::<Result<Vec<_>, _>>()?;

    let constants = template
        .constants()
        .iter()
        .map(|v| from_value_inner(*v, ctx))
        .collect::<Result<Vec<_>, _>>()?;

    // The LIR crosses with the closure, so the worker's JIT can compile it.
    let (lir, lir_values) = send_lir(template.lir(), ctx)?;

    // The nested lambdas' code objects cross beside the closure's, so the
    // receiver fills the child table its `MakeClosure`s index.
    let child_protos = send_children(template, ctx)?;

    ctx.closures[idx] = SendableClosure {
        constants,
        lir,
        lir_values,
        child_protos,
        ..code_fields(template, env, closure_rc.squelch_mask)
    };

    Ok(SendValue::Ref(idx))
}
