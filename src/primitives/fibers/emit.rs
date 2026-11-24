// audited: 2026-11-21
//! The `fiber/emit` native, and the definition a denied literal `emit` names.
//!
//! docs/signals/emit.md

use super::*;

/// (emit bits value) → suspends
///
/// Emit a signal from the current fiber. The signal bits and value are
/// returned directly; the VM's `handle_primitive_signal` classifies the bits,
/// stores them with the value in `fiber.signal`, and parks or raises.
pub(crate) fn prim_emit(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let bits = match resolve_signal_bits(&args[0], "emit", ctx) {
        Ok(bits) => bits,
        Err(err) => return err,
    };
    (bits, args[1])
}

/// The `fiber/emit` definition, which a denied literal `emit` names in its
/// denial payload, so the parent reads one payload from either form and can
/// raise the signal itself through `:func`. A native value names its
/// definition by address, so this is the table's own entry.
pub(crate) fn emit_def() -> &'static crate::primitives::def::PrimitiveDef {
    PRIMITIVES
        .iter()
        .find(|def| def.name == "fiber/emit")
        .expect("VM bug: the fiber primitive table lost fiber/emit")
}
