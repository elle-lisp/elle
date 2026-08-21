//! Signal-bits resolution for fiber primitives.
//!
//! Fiber primitives accept signal specifications in many surface forms
//! (integer, keyword, or any keyword collection). Centralizing the coercion
//! here keeps `fiber/new`, `emit`, and the meta ops (`squelch`, `attune`) all
//! speaking the same lookup rules against the global signal registry.

use super::*;

/// Return a keyword Value for a FiberStatus.
pub(super) fn status_keyword(status: FiberStatus) -> Value {
    Value::keyword(status.as_str())
}

/// Resolve a slice of Values (from array) to SignalBits by OR-ing keyword bits.
///
/// `context` is used in error messages (e.g., "fiber/new", "fiber/signal").
fn resolve_keyword_slice(
    elems: &[Value],
    context: &str,
    ctx: &mut NativeCtx,
) -> Result<SignalBits, (SignalBits, Value)> {
    let reg = crate::signals::registry::global_registry().lock().unwrap();
    let mut bits = SignalBits::EMPTY;
    for elem in elems {
        let name = elem.as_keyword_name().ok_or_else(|| {
            (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    format!(
                        "{}: array elements must be keywords, got {}",
                        context,
                        elem.type_name()
                    ),
                ),
            )
        })?;
        let b = reg.to_signal_bits(&name).ok_or_else(|| {
            (
                SIG_ERROR,
                ctx.error(
                    "signal-error",
                    format!("{}: unknown signal keyword :{}", context, name),
                ),
            )
        })?;
        bits = bits.union(b);
    }
    Ok(bits)
}

/// Resolve a Value to SignalBits.
///
/// Accepts these forms:
/// - Integer: passthrough as `SignalBits(value as u32)`
/// - Keyword: lookup in global registry, return `SignalBits(1 << bit_position)`
/// - Set / array / mutable array / list of keywords: look up each, OR the bits
///
/// `context` is used in error messages (e.g., "fiber/new", "fiber/signal").
pub(crate) fn resolve_signal_bits(
    val: &Value,
    context: &str,
    ctx: &mut NativeCtx,
) -> Result<SignalBits, (SignalBits, Value)> {
    // 1. Integer passthrough (existing behavior)
    if let Some(i) = val.as_int() {
        return Ok(SignalBits::from_i64(i));
    }

    // 2. Single keyword
    if let Some(name) = val.as_keyword_name() {
        let reg = crate::signals::registry::global_registry().lock().unwrap();
        return match reg.to_signal_bits(&name) {
            Some(bits) => Ok(bits),
            None => Err((
                SIG_ERROR,
                ctx.error(
                    "signal-error",
                    format!("{}: unknown signal keyword :{}", context, name),
                ),
            )),
        };
    }

    // 3. Set of keywords
    if let Some(set) = val.as_set() {
        let reg = crate::signals::registry::global_registry().lock().unwrap();
        let mut bits = SignalBits::EMPTY;
        for elem in set.iter() {
            let name = elem.as_keyword_name().ok_or_else(|| {
                (
                    SIG_ERROR,
                    ctx.error(
                        "type-error",
                        format!(
                            "{}: set elements must be keywords, got {}",
                            context,
                            elem.type_name()
                        ),
                    ),
                )
            })?;
            let b = reg.to_signal_bits(&name).ok_or_else(|| {
                (
                    SIG_ERROR,
                    ctx.error(
                        "signal-error",
                        format!("{}: unknown signal keyword :{}", context, name),
                    ),
                )
            })?;
            bits = bits.union(b);
        }
        return Ok(bits);
    }

    // 4. Array of keywords (immutable [...])
    if let Some(elems) = val.as_array() {
        return resolve_keyword_slice(elems, context, ctx);
    }

    // 5. Mutable array of keywords (@[...])
    if let Some(arr) = val.as_array_mut() {
        let elems = arr.borrow();
        return resolve_keyword_slice(&elems, context, ctx);
    }

    // 6. List of keywords (pair chain)
    if val.as_pair().is_some() {
        let reg = crate::signals::registry::global_registry().lock().unwrap();
        let mut bits = SignalBits::EMPTY;
        let mut current = *val;
        while let Some(pair) = current.as_pair() {
            let name = pair.first.as_keyword_name().ok_or_else(|| {
                (
                    SIG_ERROR,
                    ctx.error(
                        "type-error",
                        format!(
                            "{}: list elements must be keywords, got {}",
                            context,
                            pair.first.type_name()
                        ),
                    ),
                )
            })?;
            let b = reg.to_signal_bits(&name).ok_or_else(|| {
                (
                    SIG_ERROR,
                    ctx.error(
                        "signal-error",
                        format!("{}: unknown signal keyword :{}", context, name),
                    ),
                )
            })?;
            bits = bits.union(b);
            current = pair.rest;
        }
        return Ok(bits);
    }

    // 7. None of the above
    Err((
        SIG_ERROR,
        ctx.error(
            "type-error",
            format!(
                "{}: expected integer, keyword, or collection of keywords, got {}",
                context,
                val.type_name()
            ),
        ),
    ))
}
