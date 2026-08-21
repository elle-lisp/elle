//! Box primitives for mutable storage
use crate::primitives::def::RegionEffect;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;

/// Create a mutable box containing a value
///
/// (box value) -> box
///
/// Creates a mutable box that can be modified with rebox
pub(crate) fn prim_box(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    (SIG_OK, ctx.lbox(args[0]))
}

/// Extract the value from a box
///
/// (unbox box) -> value
///
/// Returns the current value stored in the box
pub(crate) fn prim_unbox(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if let Some(v) = args[0].lbox_get() {
        (SIG_OK, v)
    } else {
        type_error!(ctx, args[0], "unbox", "box")
    }
}

/// Modify the value in a box
///
/// (rebox box value) -> value
///
/// Sets the box to contain the new value and returns the new value
pub(crate) fn prim_rebox(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if args[0].is_lbox() {
        crate::value::arena::lbox_store_with_rebind(ctx.heap_mut(), args[0], args[1]);
        (SIG_OK, args[1])
    } else {
        type_error!(ctx, args[0], "rebox", "box")
    }
}

primitive! {
    "box" => prim_box {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Create a mutable box containing a value.",
        params: &["value"],
        category: "box",
        example: "(box 42) #=> #<box>",
        effect: RegionEffect::Fresh,
    }
    "unbox" => prim_unbox {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Extract the value from a box.",
        params: &["box"],
        category: "box",
        example: "(unbox (box 42)) #=> 42",
        effect: RegionEffect::PassThrough,
    }
    "rebox" => prim_rebox {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Modify the value in a box and return the new value.",
        params: &["box", "value"],
        category: "box",
        example: "(let [c (box 1)] (rebox c 2) (unbox c)) #=> 2",
        effect: RegionEffect::PassThrough,
    }
}
