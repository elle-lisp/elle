//! %-intrinsic NativeFn primitives.
//!
//! Each %-intrinsic is registered as a real `NativeFn` with `Signal::silent()`
//! — the op's value-position face: a bare `%add` passed to a HOF or called
//! dynamically validates its arguments here at runtime and returns
//! `(SIG_ERROR, error_val(...))` on mismatch. The storing/removing/copying ops
//! (`IntrinsicOp::routes_native_funnel()`) are additionally the funnel natives
//! every compiled call-position use lowers to (docs/intrinsics.md § Lowering);
//! the other ops lower to inline BinOp/CmpOp/... instructions in call position
//! and reach these functions only as values.

use crate::arithmetic;
use crate::primitives::ctx::NativeCtx;
use crate::primitives::def::{RegionEffect, RetType};
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;

mod data;
mod num;
pub(crate) use data::*;
use num::*;

// ── Helpers ─────────────────────────────────────────────────────────

fn type_err(name: &str, expected: &str, got: &Value, ctx: &mut NativeCtx) -> (SignalBits, Value) {
    (
        SIG_ERROR,
        ctx.error(
            "type-error",
            format!("{}: expected {}, got {}", name, expected, got.type_name()),
        ),
    )
}

fn type_err2(
    name: &str,
    expected: &str,
    a: &Value,
    b: &Value,
    ctx: &mut NativeCtx,
) -> (SignalBits, Value) {
    (
        SIG_ERROR,
        ctx.error(
            "type-error",
            format!(
                "{}: expected {}, got {} and {}",
                name,
                expected,
                a.type_name(),
                b.type_name()
            ),
        ),
    )
}

// ── Arithmetic ──────────────────────────────────────────────────────

// ── Registration table ──────────────────────────────────────────────

primitive! {
    "%add" => prim_add {
        arity: Arity::Exact(2),
        doc: "Add two numbers",
        params: &["a", "b"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%sub" => prim_sub {
        arity: Arity::Range(1, 2),
        doc: "Subtract or negate",
        params: &["a", "b?"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%mul" => prim_mul {
        arity: Arity::Exact(2),
        doc: "Multiply two numbers",
        params: &["a", "b"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%div" => prim_div {
        arity: Arity::Exact(2),
        doc: "Divide two numbers",
        params: &["a", "b"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%rem" => prim_rem {
        arity: Arity::Exact(2),
        doc: "Remainder (sign follows dividend)",
        params: &["a", "b"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%mod" => prim_mod {
        arity: Arity::Exact(2),
        doc: "Floored modulus (sign follows divisor)",
        params: &["a", "b"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%eq" => prim_eq {
        arity: Arity::Exact(2),
        doc: "Equality",
        params: &["a", "b"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%ne" => prim_ne {
        arity: Arity::Exact(2),
        doc: "Not equal",
        params: &["a", "b"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%lt" => prim_lt {
        arity: Arity::Exact(2),
        doc: "Less than",
        params: &["a", "b"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%gt" => prim_gt {
        arity: Arity::Exact(2),
        doc: "Greater than",
        params: &["a", "b"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%le" => prim_le {
        arity: Arity::Exact(2),
        doc: "Less than or equal",
        params: &["a", "b"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%ge" => prim_ge {
        arity: Arity::Exact(2),
        doc: "Greater than or equal",
        params: &["a", "b"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%not" => prim_not {
        arity: Arity::Exact(1),
        doc: "Logical not",
        params: &["x"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%int" => prim_int {
        arity: Arity::Exact(1),
        doc: "Convert to integer",
        params: &["x"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%float" => prim_float {
        arity: Arity::Exact(1),
        doc: "Convert to float",
        params: &["x"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%pair" => prim_pair {
        arity: Arity::Exact(2),
        doc: "Construct a pair",
        params: &["a", "b"],
        category: "intrinsic",
        effect: RegionEffect::Fresh,
    }
    "%first" => prim_first {
        arity: Arity::Exact(1),
        doc: "First of pair",
        params: &["p"],
        category: "intrinsic",
        effect: RegionEffect::PassThrough,
    }
    "%rest" => prim_rest {
        arity: Arity::Exact(1),
        doc: "Rest of pair",
        params: &["p"],
        category: "intrinsic",
        effect: RegionEffect::PassThrough,
    }
    "%bit-and" => prim_bit_and {
        arity: Arity::Exact(2),
        doc: "Bitwise AND",
        params: &["a", "b"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%bit-or" => prim_bit_or {
        arity: Arity::Exact(2),
        doc: "Bitwise OR",
        params: &["a", "b"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%bit-xor" => prim_bit_xor {
        arity: Arity::Exact(2),
        doc: "Bitwise XOR",
        params: &["a", "b"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%bit-not" => prim_bit_not {
        arity: Arity::Exact(1),
        doc: "Bitwise complement",
        params: &["x"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%shl" => prim_shl {
        arity: Arity::Exact(2),
        doc: "Shift left",
        params: &["a", "b"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%shr" => prim_shr {
        arity: Arity::Exact(2),
        doc: "Shift right",
        params: &["a", "b"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%nil?" => prim_nil_q {
        arity: Arity::Exact(1),
        doc: "Is nil?",
        params: &["x"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%empty?" => prim_empty_q {
        arity: Arity::Exact(1),
        doc: "Is empty list?",
        params: &["x"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%bool?" => prim_bool_q {
        arity: Arity::Exact(1),
        doc: "Is boolean?",
        params: &["x"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%int?" => prim_int_q {
        arity: Arity::Exact(1),
        doc: "Is integer?",
        params: &["x"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%float?" => prim_float_q {
        arity: Arity::Exact(1),
        doc: "Is float?",
        params: &["x"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%string?" => prim_string_q {
        arity: Arity::Exact(1),
        doc: "Is string?",
        params: &["x"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%keyword?" => prim_keyword_q {
        arity: Arity::Exact(1),
        doc: "Is keyword?",
        params: &["x"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%symbol?" => prim_symbol_q {
        arity: Arity::Exact(1),
        doc: "Is symbol?",
        params: &["x"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%pair?" => prim_pair_q {
        arity: Arity::Exact(1),
        doc: "Is pair?",
        params: &["x"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%array?" => prim_array_q {
        arity: Arity::Exact(1),
        doc: "Is array?",
        params: &["x"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%struct?" => prim_struct_q {
        arity: Arity::Exact(1),
        doc: "Is struct?",
        params: &["x"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%set?" => prim_set_q {
        arity: Arity::Exact(1),
        doc: "Is set?",
        params: &["x"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%bytes?" => prim_bytes_q {
        arity: Arity::Exact(1),
        doc: "Is bytes?",
        params: &["x"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%box?" => prim_box_q {
        arity: Arity::Exact(1),
        doc: "Is box?",
        params: &["x"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%closure?" => prim_closure_q {
        arity: Arity::Exact(1),
        doc: "Is closure?",
        params: &["x"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%fiber?" => prim_fiber_q {
        arity: Arity::Exact(1),
        doc: "Is fiber?",
        params: &["x"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%type-of" => prim_type_of {
        arity: Arity::Exact(1),
        doc: "Type as keyword",
        params: &["x"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%length" => prim_length {
        arity: Arity::Exact(1),
        doc: "Polymorphic length",
        params: &["x"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%get" => prim_get {
        arity: Arity::Range(2, 3),
        doc: "Indexed/keyed access",
        params: &["coll", "key"],
        category: "intrinsic",
        effect: RegionEffect::Funnel,
    }
    "%put" => prim_put {
        arity: Arity::Range(2, 3),
        doc: "Assoc/set element",
        params: &["coll", "key", "val"],
        category: "intrinsic",
        // Funnel, not Mixed: the mutable-path store goes through the
        // arena funnels (struct_put/set_at/set_add — runtime-counted);
        // the immutable path returns a fresh alloc-scan-counted copy.
        // The result is genuinely mixed (fresh copy vs container), but
        // no store is uncounted, so no clique edges.
        effect: RegionEffect::Funnel,
    }
    // Monomorphic put twins. Same runtime body (prim_put) as polymorphic %put;
    // precise RetType is the monomorphization win. Effect stays Funnel (no
    // result-side oracle constraint): the same NativeFn also serves dynamic
    // value-position calls, where no compile-time proof constrains the input
    // mutability, so the result is still conditionally fresh. Registration
    // makes them the funnel natives compiled call-position uses lower to
    // (`routes_native_funnel`) and bare callable values.
    "%put-struct" => prim_put {
        arity: Arity::Exact(3),
        doc: "Assoc into an immutable struct, returning a fresh struct (monomorphic immutable struct put)",
        params: &["coll", "key", "val"],
        category: "intrinsic",
        effect: RegionEffect::Funnel,
        ret: RetType::Struct,
    }
    "%put-struct-mut" => prim_put {
        arity: Arity::Exact(3),
        doc: "Assoc into a mutable @struct in place, returning it (monomorphic @struct put)",
        params: &["coll", "key", "val"],
        category: "intrinsic",
        effect: RegionEffect::Funnel,
        ret: RetType::MutableStruct,
    }
    "%put-array" => prim_put {
        arity: Arity::Exact(3),
        doc: "Set an index in an immutable array, returning a fresh array (monomorphic immutable array put)",
        params: &["coll", "key", "val"],
        category: "intrinsic",
        effect: RegionEffect::Funnel,
        ret: RetType::Array,
    }
    "%put-array-mut" => prim_put {
        arity: Arity::Exact(3),
        doc: "Set an index in a mutable @array in place, returning it (monomorphic @array put)",
        params: &["coll", "key", "val"],
        category: "intrinsic",
        effect: RegionEffect::Funnel,
        ret: RetType::MutableArray,
    }
    // Monomorphic set-add twins. Same runtime body (prim_add_set → sets::prim_add)
    // as the polymorphic set `add`; the NativeFn registration makes them the funnel
    // natives compiled call-position uses lower to (`routes_native_funnel`) and bare
    // callable values. RetType is the monomorphization win: %add-set is a fresh
    // immutable Set, %add-set-mut its mutable arg0. Effect stays Funnel (the same
    // NativeFn serves dynamic value-position calls, where no compile-time proof
    // constrains the input mutability), like the %put/%push mut twins.
    "%add-set" => prim_add_set {
        arity: Arity::Exact(2),
        doc: "Add to an immutable set, returning a fresh set (monomorphic immutable set add)",
        params: &["set", "value"],
        category: "intrinsic",
        effect: RegionEffect::Funnel,
        ret: RetType::Set,
    }
    "%add-set-mut" => prim_add_set {
        arity: Arity::Exact(2),
        doc: "Add to a mutable @set in place, returning it (monomorphic @set add)",
        params: &["set", "value"],
        category: "intrinsic",
        effect: RegionEffect::Funnel,
        ret: RetType::MutableSet,
    }
    "%del" => prim_del {
        arity: Arity::Exact(2),
        doc: "Dissoc/delete key",
        params: &["coll", "key"],
        category: "intrinsic",
        effect: RegionEffect::Funnel,
    }
    "%has?" => prim_has {
        arity: Arity::Exact(2),
        doc: "Key/element exists?",
        params: &["coll", "key"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
    "%array-push" => prim_push {
        arity: Arity::Exact(2),
        doc: "Append element",
        params: &["arr", "val"],
        category: "intrinsic",
        // Funnel: the @array store goes through the arena push funnel
        // (runtime-counted); the immutable path returns a fresh copy.
        effect: RegionEffect::Funnel,
    }
    // Monomorphic array-push twins. Same runtime body (prim_push) as the
    // polymorphic %array-push; what differs is the *static* declaration. The
    // NativeFn registration makes them the funnel natives compiled
    // call-position uses lower to (`routes_native_funnel`) and bare callable
    // values (HOF). RetType is the monomorphization win already available:
    // %push-array is a fresh immutable Array, %push-array-mut its mutable
    // arg0. Effect stays Funnel (identical to %array-push, and Funnel carries
    // no result-side oracle constraint): the same NativeFn also serves dynamic
    // value-position calls, where no compile-time proof constrains the input
    // mutability, so the precise Fresh-vs-funnel split stays unsound here.
    "%push-array" => prim_push {
        arity: Arity::Exact(2),
        doc: "Append to an immutable array, returning a fresh array (monomorphic immutable array-push)",
        params: &["arr", "val"],
        category: "intrinsic",
        effect: RegionEffect::Funnel,
        ret: RetType::Array,
    }
    "%push-array-mut" => prim_push {
        arity: Arity::Exact(2),
        doc: "Append to a mutable @array in place, returning it (monomorphic @array push)",
        params: &["arr", "val"],
        category: "intrinsic",
        effect: RegionEffect::Funnel,
        ret: RetType::MutableArray,
    }
    "%pop" => prim_pop {
        arity: Arity::Exact(1),
        doc: "Remove/return last element",
        params: &["arr"],
        category: "intrinsic",
        // PassThrough: the result lives in the popped element's region, not the
        // call's own. moves_out: that element is REMOVED from the @array, so its
        // pass-through retain is taken in-body BEFORE the container's reference is
        // released (dispatch skips its own — see `moves_out`).
        effect: RegionEffect::PassThrough,
        moves_out: true,
    }
    "%string-push" => prim_string_push {
        arity: Arity::Exact(2),
        doc: "Append string to string/@string",
        params: &["s", "val"],
        category: "intrinsic",
        // Funnel, not PassThrough: like %put/%array-push this is
        // conditionally allocating — the @string path appends in place
        // (pass-through), the immutable string path returns a FRESH copy in
        // this call's own region. Declaring PassThrough would trip the
        // dispatch_native_call declaration oracle on the immutable case,
        // since the op routes as a real native Call.
        effect: RegionEffect::Funnel,
    }
    "%bytes-push" => prim_bytes_push {
        arity: Arity::Exact(2),
        doc: "Append a byte (integer) or all bytes of a bytes value to bytes/@bytes",
        params: &["b", "val"],
        category: "intrinsic",
        // Funnel, not PassThrough: @bytes appends in place (pass-through),
        // the immutable bytes path returns a FRESH copy — same conditional
        // allocation as %string-push/%put.
        effect: RegionEffect::Funnel,
    }
    "%freeze" => prim_freeze {
        arity: Arity::Exact(1),
        doc: "Mutable to immutable",
        params: &["x"],
        category: "intrinsic",
        effect: RegionEffect::Funnel,
    }
    "%thaw" => prim_thaw {
        arity: Arity::Exact(1),
        doc: "Immutable to mutable",
        params: &["x"],
        category: "intrinsic",
        effect: RegionEffect::Funnel,
    }
    "%identical?" => prim_identical {
        arity: Arity::Exact(2),
        doc: "Pointer identity",
        params: &["a", "b"],
        category: "intrinsic",
        effect: RegionEffect::Immediate,
    }
}

#[cfg(test)]
mod tests;
