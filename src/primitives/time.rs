use crate::primitives::def::RegionEffect;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;
use std::sync::OnceLock;
use std::time::Instant;

static PROCESS_EPOCH: OnceLock<Instant> = OnceLock::new();

fn process_epoch() -> &'static Instant {
    PROCESS_EPOCH.get_or_init(Instant::now)
}

/// Returns seconds elapsed since process start (monotonic clock)
/// (clock/monotonic)
pub(crate) fn prim_clock_monotonic(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    (
        SIG_OK,
        Value::float(process_epoch().elapsed().as_secs_f64()),
    )
}

/// Returns thread CPU time in seconds
/// (clock/cpu)
pub(crate) fn prim_clock_cpu(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: clock_gettime with CLOCK_THREAD_CPUTIME_ID is always valid
    // and ts is a properly initialized timespec.
    let ret = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) };
    if ret != 0 {
        return (
            SIG_ERROR,
            ctx.error("io-error", "clock/cpu: clock_gettime failed".to_string()),
        );
    }
    let secs = ts.tv_sec as f64 + ts.tv_nsec as f64 / 1_000_000_000.0;
    (SIG_OK, Value::float(secs))
}

/// Returns seconds since Unix epoch (wall clock)
/// (clock/realtime)
pub(crate) fn prim_clock_realtime(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(duration) => (SIG_OK, Value::float(duration.as_secs_f64())),
        Err(_) => (
            SIG_ERROR,
            ctx.error(
                "io-error",
                "clock/realtime: system clock is before Unix epoch".to_string(),
            ),
        ),
    }
}

/// Sleeps for the specified number of seconds
/// (time/sleep seconds)
pub(crate) fn prim_sleep(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if let Some(n) = args[0].as_int() {
        if n < 0 {
            return (
                SIG_ERROR,
                ctx.error(
                    "argument-error",
                    "time/sleep: duration must be non-negative".to_string(),
                ),
            );
        }
        std::thread::sleep(std::time::Duration::from_secs(n as u64));
        (SIG_OK, Value::NIL)
    } else if let Some(f) = args[0].as_float() {
        if f < 0.0 || !f.is_finite() {
            return (
                SIG_ERROR,
                ctx.error(
                    "argument-error",
                    "time/sleep: duration must be a finite non-negative number".to_string(),
                ),
            );
        }
        std::thread::sleep(std::time::Duration::from_secs_f64(f));
        (SIG_OK, Value::NIL)
    } else {
        (
            SIG_ERROR,
            ctx.error(
                "type-error",
                "time/sleep: argument must be a number".to_string(),
            ),
        )
    }
}

// Declarative primitive definitions for time operations
primitive! {
    "clock/monotonic" => prim_clock_monotonic {
        signal: Signal::errors(),
        doc: "Return seconds elapsed since process start (monotonic clock)",
        category: "clock",
        example: "(clock/monotonic)",
        effect: RegionEffect::Immediate,
    }
    "clock/realtime" => prim_clock_realtime {
        signal: Signal::errors(),
        doc: "Return seconds since Unix epoch (wall clock)",
        category: "clock",
        example: "(clock/realtime)",
        effect: RegionEffect::Immediate,
    }
    "clock/cpu" => prim_clock_cpu {
        signal: Signal::errors(),
        doc: "Return thread CPU time in seconds",
        category: "clock",
        example: "(clock/cpu)",
        effect: RegionEffect::Immediate,
    }
    "time/sleep" => prim_sleep {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Sleep for the specified number of seconds (blocks the thread)",
        params: &["seconds"],
        category: "time",
        example: "(time/sleep 1.5)",
        effect: RegionEffect::Immediate,
    }
}
