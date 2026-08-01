//! Argument decoding for native primitives and VM opcode handlers.
//!
//! A primitive receives `args: &[Value]` — untyped, already arity-checked by
//! the dispatcher. Nearly every one starts by narrowing an argument to a
//! concrete type and reporting a `type-error` when it does not fit. These
//! macros carry that step so the message format has one definition.
//!
//! The message a caller sees is `"<primitive>: expected <type>, got <actual>"`.
//! Each site names its own primitive, because a primitive cannot see the name
//! it was registered under; the accessor and the type word are what change
//! between sites.
//!
//! The opcode handlers in `vm` report the same mismatches and so share the
//! format from here, through `vm_type_error!`, even though they signal
//! through the VM rather than by returning a pair.

/// Narrow `args[$idx]` through a `Value::as_*` accessor, or return a
/// `type-error` from the enclosing primitive.
///
/// Expands to the value on success. On failure it *returns* from the calling
/// function with `(SIG_ERROR, error)`, so it only appears in a primitive whose
/// return type is `(SignalBits, Value)`.
///
/// ```ignore
/// let handle = prim_arg!(ctx, args, 0, as_fiber, "fiber/bits", "fiber");
/// let n = prim_arg!(ctx, args, 1, as_int, "fiber/set-fuel", "integer");
/// ```
macro_rules! prim_arg {
    ($ctx:expr, $args:expr, $idx:expr, $as:ident, $prim:literal, $want:literal) => {
        match $args[$idx].$as() {
            Some(v) => v,
            None => {
                return (
                    $crate::value::SIG_ERROR,
                    $ctx.error(
                        "type-error",
                        format!(
                            concat!($prim, ": expected ", $want, ", got {}"),
                            $args[$idx].type_name()
                        ),
                    ),
                );
            }
        }
    };
}

/// The `(SIG_ERROR, error)` pair a primitive yields for a type mismatch on
/// `$val`, as an expression.
///
/// For the arguments no single accessor decides — a primitive accepting either
/// an `@array` or an `@string`, say, which tries each in turn and reports the
/// failure once at the end — and for the primitives that produce the pair as a
/// tail expression rather than returning early.
///
/// ```ignore
/// type_error!(ctx, args[0], "popn", "@array or @string")
/// ```
macro_rules! type_error {
    ($ctx:expr, $val:expr, $prim:literal, $want:literal) => {
        (
            $crate::value::SIG_ERROR,
            $ctx.error(
                "type-error",
                format!(
                    concat!($prim, ": expected ", $want, ", got {}"),
                    $val.type_name()
                ),
            ),
        )
    };
}

/// Report a type mismatch from a VM opcode handler, push nil as the opcode's
/// result, and return.
///
/// The opcode handlers do not return `(SignalBits, Value)` like a primitive:
/// they signal through `vm.set_error` and must still leave a value on the
/// operand stack, because the instruction that follows expects the slot to be
/// filled whatever happened here.
///
/// ```ignore
/// let Some(idx) = idx_val.as_int() else {
///     vm_type_error!(vm, idx_val, "array-ref", "integer index")
/// };
/// ```
macro_rules! vm_type_error {
    ($vm:expr, $val:expr, $prim:literal, $want:literal) => {{
        $vm.set_error(
            "type-error",
            format!(
                concat!($prim, ": expected ", $want, ", got {}"),
                $val.type_name()
            ),
        );
        $vm.fiber.stack.push($crate::value::Value::NIL);
        return;
    }};
}

/// The same pair as [`type_error`], for a primitive whose name is known only
/// at run time.
///
/// A few helpers serve a whole family of primitives and receive the name as a
/// parameter — `unary_float` covers `sqrt`, `sin`, `cos` and the rest. The
/// macros need a literal to build the message at compile time, so those
/// helpers format it here instead.
pub(crate) fn type_error_named(
    ctx: &crate::primitives::ctx::NativeCtx<'_>,
    val: &crate::value::Value,
    prim: &str,
    want: &str,
) -> (crate::value::SignalBits, crate::value::Value) {
    (
        crate::value::SIG_ERROR,
        ctx.error(
            "type-error",
            format!("{prim}: expected {want}, got {}", val.type_name()),
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::type_error_named;
    use crate::primitives::ctx::{with_test_ctx, NativeCtx};
    use crate::value::{SignalBits, Value, SIG_ERROR, SIG_OK};

    /// A primitive shaped exactly like the real ones, so the macro is exercised
    /// through the `return` it performs rather than as an expression.
    fn takes_an_int(ctx: &mut NativeCtx<'_>, args: &[Value]) -> (SignalBits, Value) {
        let n = prim_arg!(ctx, args, 0, as_int, "demo/takes-int", "integer");
        (SIG_OK, Value::int(n * 2))
    }

    fn rejects_outright(ctx: &mut NativeCtx<'_>, args: &[Value]) -> (SignalBits, Value) {
        type_error!(ctx, args[0], "demo/rejects", "@array or @string")
    }

    #[test]
    fn a_matching_argument_decodes_and_execution_continues() {
        with_test_ctx(|ctx| {
            let (bits, val) = takes_an_int(ctx, &[Value::int(21)]);
            assert_eq!(bits, SIG_OK);
            assert_eq!(val.as_int(), Some(42));
        });
    }

    #[test]
    fn a_mismatched_argument_returns_a_type_error_naming_the_primitive() {
        with_test_ctx(|ctx| {
            let (bits, val) = takes_an_int(ctx, &[Value::bool(true)]);
            assert_eq!(bits, SIG_ERROR, "a type mismatch must signal an error");
            let msg = format!("{}", val);
            assert!(
                msg.contains("demo/takes-int: expected integer, got bool"),
                "message should name the primitive, the wanted type, and the \
                 actual type; got: {msg}"
            );
        });
    }

    #[test]
    fn the_runtime_name_variant_matches_the_macro_word_for_word() {
        // The two must stay interchangeable: a family helper that formats its
        // own name should be indistinguishable from a primitive that spells
        // the name out, or the same mistake reads differently depending on
        // which primitive hit it.
        with_test_ctx(|ctx| {
            let (bits, val) = takes_an_int(ctx, &[Value::bool(true)]);
            let from_macro = format!("{val}");
            assert_eq!(bits, SIG_ERROR);

            let (bits, val) =
                type_error_named(ctx, &Value::bool(true), "demo/takes-int", "integer");
            assert_eq!(bits, SIG_ERROR);
            assert_eq!(format!("{val}"), from_macro);
        });
    }

    #[test]
    fn the_outright_rejection_reports_the_same_shape() {
        with_test_ctx(|ctx| {
            let (bits, val) = rejects_outright(ctx, &[Value::int(1)]);
            assert_eq!(bits, SIG_ERROR);
            let msg = format!("{}", val);
            assert!(
                msg.contains("demo/rejects: expected @array or @string, got int"),
                "got: {msg}"
            );
        });
    }
}
