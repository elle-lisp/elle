// audited: 2026-10-21
//! Serializing a live closure instance into the bundle's intern table.
//!
//! docs/threads.md
//!
//! Split from the value-tag `match` because the closure arm manages cycle
//! detection: it pre-inserts a placeholder before recursing into `env`, the
//! constants and the LIR, and it carries the full nested-lambda tree.
//! Isolating it keeps the tag dispatch readable.

use super::super::*;
use super::ctx::SerContext;
use super::from_value_inner;
use super::template::{code_fields, sendable};

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

    // The constants, the LIR the worker's JIT compiles, and the nested
    // lambdas' code objects the receiver's `MakeClosure`s index all cross
    // beside the closure.
    ctx.closures[idx] = sendable(template, env, closure_rc.squelch_mask, ctx)?;

    Ok(SendValue::Ref(idx))
}
