use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};
use std::sync::OnceLock;
use std::time::Instant;

static PROCESS_EPOCH: OnceLock<Instant> = OnceLock::new();

fn process_epoch() -> &'static Instant {
    PROCESS_EPOCH.get_or_init(Instant::now)
}

/// Returns seconds elapsed since process start (monotonic clock)
/// (clock/monotonic)
pub fn prim_clock_monotonic(args: &[Value]) -> (SignalBits, Value) {
    if !args.is_empty() {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("clock/monotonic: expected 0 arguments, got {}", args.len()),
            ),
        );
    }

    (
        SIG_OK,
        Value::float(process_epoch().elapsed().as_secs_f64()),
    )
}

/// Returns thread CPU time in seconds
/// (clock/cpu)
pub fn prim_clock_cpu(args: &[Value]) -> (SignalBits, Value) {
    if !args.is_empty() {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("clock/cpu: expected 0 arguments, got {}", args.len()),
            ),
        );
    }

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
            error_val("error", "clock/cpu: clock_gettime failed".to_string()),
        );
    }
    let secs = ts.tv_sec as f64 + ts.tv_nsec as f64 / 1_000_000_000.0;
    (SIG_OK, Value::float(secs))
}

/// Returns seconds since Unix epoch (wall clock)
/// (clock/realtime)
pub fn prim_clock_realtime(args: &[Value]) -> (SignalBits, Value) {
    if !args.is_empty() {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("clock/realtime: expected 0 arguments, got {}", args.len()),
            ),
        );
    }

    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(duration) => (SIG_OK, Value::float(duration.as_secs_f64())),
        Err(_) => (
            SIG_ERROR,
            error_val(
                "error",
                "clock/realtime: system clock is before Unix epoch".to_string(),
            ),
        ),
    }
}

/// Sleeps for the specified number of seconds
/// (time/sleep seconds)
pub fn prim_sleep(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("time/sleep: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    if let Some(n) = args[0].as_int() {
        if n < 0 {
            return (
                SIG_ERROR,
                error_val(
                    "error",
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
                error_val(
                    "error",
                    "time/sleep: duration must be a finite non-negative number".to_string(),
                ),
            );
        }
        std::thread::sleep(std::time::Duration::from_secs_f64(f));
        (SIG_OK, Value::NIL)
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                "time/sleep: argument must be a number".to_string(),
            ),
        )
    }
}

/// Declarative primitive definitions for time operations
pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "clock/monotonic",
        func: prim_clock_monotonic,
        effect: Effect::none(),
        arity: Arity::Exact(0),
        doc: "Return seconds elapsed since process start (monotonic clock)",
        params: &[],
        category: "clock",
        example: "(clock/monotonic)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "clock/realtime",
        func: prim_clock_realtime,
        effect: Effect::none(),
        arity: Arity::Exact(0),
        doc: "Return seconds since Unix epoch (wall clock)",
        params: &[],
        category: "clock",
        example: "(clock/realtime)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "clock/cpu",
        func: prim_clock_cpu,
        effect: Effect::none(),
        arity: Arity::Exact(0),
        doc: "Return thread CPU time in seconds",
        params: &[],
        category: "clock",
        example: "(clock/cpu)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "time/sleep",
        func: prim_sleep,
        effect: Effect::raises(),
        arity: Arity::Exact(1),
        doc: "Sleep for the specified number of seconds",
        params: &["seconds"],
        category: "time",
        example: "(time/sleep 1.5)",
        aliases: &[],
    },
];
