//! Fiber fuel (instruction-budget) primitives.
//!
//! Fuel bounds how long a fiber runs before it must yield control, which is how
//! a scheduler enforces fairness / preemption. These three ops set, read, and
//! clear that budget; separated from lifecycle handlers because they touch only
//! the `fuel` field and never the fiber's signal/status machinery.

use super::*;

/// (fiber/set-fuel fiber n) → nil
///
/// Set the instruction budget on a fiber. `n` must be a non-negative integer.
/// A fuel of 0 means the very next fuel checkpoint emits `:fuel`.
pub(crate) fn prim_fiber_set_fuel(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let handle = prim_arg!(ctx, args, 0, as_fiber, "fiber/set-fuel", "fiber");

    let fuel = match args[1].as_int() {
        Some(n) if n >= 0 => n as u32,
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
        fiber.fuel = Some(fuel);
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
            .map(|f| Value::int(f as i64))
            .unwrap_or(Value::NIL)
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
        fiber.fuel = None;
    });

    (SIG_OK, Value::NIL)
}
