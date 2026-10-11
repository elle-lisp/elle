// audited: 2026-10-05
//! The entry check of a `(bound p spec)` bound, asked by the interpreter's
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
/// one. A closure that passes on its arguments' signals fails whatever its
/// bits say, because those bits are only a lower bound. Any other value
/// passes: it carries no signal, and calling it raises inside the function,
/// which the `:error` every bounded function carries covers. Both tiers ask
/// here, so they cannot disagree on what the bound admits.
pub fn violation(value: Value, allowed: SignalBits) -> Option<String> {
    let (what, signal) = if let Some(closure) = value.as_closure() {
        ("closure".to_string(), closure.signal())
    } else {
        let def = value.as_native_def()?;
        (def.name.to_string(), def.signal)
    };
    if signal.is_polymorphic() {
        return Some(format!(
            "bound: {} passes on the signal of its arguments but parameter is bounded to {}",
            what,
            registry::format_bits(allowed)
        ));
    }
    let bits = signal.bits;
    let excess = bits.subtract(allowed);
    if excess.is_empty() {
        return None;
    }
    Some(format!(
        "bound: {} may emit {} but parameter is bounded to {}",
        what,
        registry::format_bits(excess),
        registry::format_bits(allowed)
    ))
}
