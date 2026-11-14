// audited: 2026-11-13
//! `fiber/signaled?`: whether a fiber's last signal shares a bit with a signal the registry names.
//!
//! docs/signals/protocol.md
//! docs/signals/primitives.md

use crate::primitives::fibers::resolve_signal_bits;
use crate::value::fiber::{SignalBits, SIG_OK};
use crate::value::Value;

/// (fiber/signaled? fiber spec) → bool
///
/// True when the fiber's last signal shares a bit with `spec`, the rule a mask
/// catches by. The registry resolves `spec`, so a keyword it does not hold
/// raises `:signal-error` rather than reading as a signal never raised.
pub(crate) fn prim_fiber_signaled(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let handle = prim_arg!(ctx, args, 0, as_fiber, "fiber/signaled?", "fiber");
    let wanted = match resolve_signal_bits(&args[1], "fiber/signaled?", ctx) {
        Ok(bits) => bits,
        Err(raised) => return raised,
    };
    let bits = handle.with(|fiber| fiber.signal.as_ref().map(|(b, _)| *b).unwrap_or(SIG_OK));
    (SIG_OK, Value::bool(bits.intersects(wanted)))
}
