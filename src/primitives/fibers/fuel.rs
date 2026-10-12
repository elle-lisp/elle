// audited: 2026-10-12
//! Fiber fuel (instruction-budget) primitives.
//!
//! docs/runtime.md
//!
//! Fuel bounds how long a fiber runs before it must yield control, which is how
//! a scheduler enforces fairness / preemption. These three ops set, read, and
//! clear that budget; separated from lifecycle handlers because they touch only
//! the `fuel` field and never the fiber's signal/status machinery.

use super::*;
use crate::value::fiber::Fuel;

/// (fiber/set-fuel fiber n) → nil
///
/// Set the instruction budget on a fiber to any non-negative integer `n`.
/// A budget of 0 pauses the fiber with `:fuel` at its next charge site outside
/// a silent function.
pub(crate) fn prim_fiber_set_fuel(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let handle = prim_arg!(ctx, args, 0, as_fiber, "fiber/set-fuel", "fiber");

    let fuel = match args[1].as_int() {
        Some(n) if n >= 0 => Fuel::budget(n as u64).expect("a non-negative budget"),
        Some(_) => {
            return (
                SIG_ERROR,
                ctx.error("type-error", "fiber/set-fuel: fuel must be non-negative"),
            );
        }
        None => {
            return type_error!(ctx, args[1], "fiber/set-fuel", "integer");
        }
    };

    handle.with_mut(|fiber| {
        fiber.fuel = fuel;
    });

    (SIG_OK, Value::NIL)
}

/// (fiber/fuel fiber) → integer | nil
///
/// Read the remaining instruction budget. Returns an integer if fuel is set,
/// or `nil` if the fiber has unlimited fuel (the default).
pub(crate) fn prim_fiber_fuel(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let handle = prim_arg!(ctx, args, 0, as_fiber, "fiber/fuel", "fiber");

    let fuel_val = handle.with(|fiber| {
        fiber
            .fuel
            .remaining()
            .map_or(Value::NIL, |n| Value::int(n as i64))
    });

    (SIG_OK, fuel_val)
}

/// (fiber/clear-fuel fiber) → nil
///
/// Remove the instruction budget, restoring unlimited execution.
pub(crate) fn prim_fiber_clear_fuel(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let handle = prim_arg!(ctx, args, 0, as_fiber, "fiber/clear-fuel", "fiber");

    handle.with_mut(|fiber| {
        fiber.fuel = Fuel::unmetered();
    });

    (SIG_OK, Value::NIL)
}
