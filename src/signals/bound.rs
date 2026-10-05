// audited: 2026-10-04
//! The entry check of a `(silence p)` bound, asked by the interpreter's
//! `CheckSignalBound` and the JIT's prologue alike.
//!
//! docs/signals/inference.md

use super::registry;
use crate::value::fiber::SignalBits;
use crate::value::Value;

/// The message the bound raises for `value`, or `None` when the value is
/// within `allowed`.
///
/// A closure is judged by its effective signal and a native by its declared
/// one. Any other value passes: it carries no signal, and calling it raises
/// inside the function, which the `:error` every bounded function carries
/// covers. Both tiers ask here, so they cannot disagree on what the bound
/// admits.
pub fn violation(value: Value, allowed: SignalBits) -> Option<String> {
    let (what, bits) = if let Some(closure) = value.as_closure() {
        ("closure".to_string(), closure.signal().bits)
    } else {
        let def = value.as_native_def()?;
        (def.name.to_string(), def.signal.bits)
    };
    let excess = bits.subtract(allowed);
    if excess.is_empty() {
        return None;
    }
    Some(format!(
        "restrict: {} may emit {} but parameter is restricted to {}",
        what,
        registry::format_bits(excess),
        registry::format_bits(allowed)
    ))
}
