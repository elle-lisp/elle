//! Formatting: temporal/string, temporal/format.
//! Epoch conversions: timestamp/->epoch, timestamp/->epoch-millis, etc.

use crate::{
    jiff_err, jiff_val, require_int, require_jiff, require_string, require_variant, JiffValue,
};
use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::Arity;
use elle::value::{error_val, Value};

/// (temporal/string val) → ISO 8601 string
fn prim_temporal_string(args: &[Value]) -> (SignalBits, Value) {
    let jv = match require_jiff(&args[0], "temporal/string") {
        Ok(jv) => jv,
        Err(e) => return e,
    };
    let s: String = match jv {
        JiffValue::Timestamp(ts) => ts.to_string(),
        JiffValue::Date(d) => d.to_string(),
        JiffValue::Time(t) => t.to_string(),
        JiffValue::DateTime(dt) => dt.to_string(),
        JiffValue::Zoned(z) => z.to_string(),
        JiffValue::Span(s) => s.to_string(),
        JiffValue::SignedDuration(d) => d.to_string(),
    };
    (SIG_OK, Value::string(s.as_str()))
}

/// (temporal/format fmt val) → formatted string (strftime)
fn prim_temporal_format(args: &[Value]) -> (SignalBits, Value) {
    let fmt = match require_string(&args[0], "temporal/format") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let jv = match require_jiff(&args[1], "temporal/format") {
        Ok(jv) => jv,
        Err(e) => return e,
    };
    let result = match jv {
        JiffValue::Timestamp(ts) => jiff::fmt::strtime::format(&fmt, *ts),
        JiffValue::Date(d) => jiff::fmt::strtime::format(&fmt, *d),
        JiffValue::Time(t) => jiff::fmt::strtime::format(&fmt, *t),
        JiffValue::DateTime(dt) => jiff::fmt::strtime::format(&fmt, *dt),
        JiffValue::Zoned(z) => jiff::fmt::strtime::format(&fmt, z.as_ref()),
        _ => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "temporal/format: cannot format {}, expected date/time type",
                        jv.type_name()
                    ),
                ),
            )
        }
    };
    match result {
        Ok(s) => (SIG_OK, Value::string(s.as_str())),
        Err(e) => jiff_err("temporal/format", e),
    }
}

// ---------------------------------------------------------------------------
// Epoch conversions
// ---------------------------------------------------------------------------

fn prim_ts_epoch(args: &[Value]) -> (SignalBits, Value) {
    let ts = match require_variant!(&args[0], Timestamp, "timestamp/->epoch", "timestamp") {
        Ok(ts) => ts,
        Err(e) => return e,
    };
    (
        SIG_OK,
        Value::float(ts.as_second() as f64 + ts.subsec_nanosecond() as f64 / 1e9),
    )
}

fn prim_ts_epoch_millis(args: &[Value]) -> (SignalBits, Value) {
    let ts = match require_variant!(&args[0], Timestamp, "timestamp/->epoch-millis", "timestamp") {
        Ok(ts) => ts,
        Err(e) => return e,
    };
    (SIG_OK, Value::int(ts.as_millisecond()))
}

fn prim_ts_epoch_micros(args: &[Value]) -> (SignalBits, Value) {
    let ts = match require_variant!(&args[0], Timestamp, "timestamp/->epoch-micros", "timestamp") {
        Ok(ts) => ts,
        Err(e) => return e,
    };
    (SIG_OK, Value::int(ts.as_microsecond()))
}

fn prim_ts_epoch_nanos(args: &[Value]) -> (SignalBits, Value) {
    let ts = match require_variant!(&args[0], Timestamp, "timestamp/->epoch-nanos", "timestamp") {
        Ok(ts) => ts,
        Err(e) => return e,
    };
    (SIG_OK, Value::int(ts.as_nanosecond() as i64))
}

fn prim_ts_from_epoch_seconds(args: &[Value]) -> (SignalBits, Value) {
    // Accept either int or float
    if let Some(n) = args[0].as_int() {
        match jiff::Timestamp::new(n, 0) {
            Ok(ts) => return (SIG_OK, jiff_val(JiffValue::Timestamp(ts))),
            Err(e) => return jiff_err("timestamp/from-epoch-seconds", e),
        }
    }
    if let Some(f) = args[0].as_float() {
        let secs = f.trunc() as i64;
        let nanos = ((f.fract()) * 1e9) as i32;
        match jiff::Timestamp::new(secs, nanos) {
            Ok(ts) => return (SIG_OK, jiff_val(JiffValue::Timestamp(ts))),
            Err(e) => return jiff_err("timestamp/from-epoch-seconds", e),
        }
    }
    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "timestamp/from-epoch-seconds: expected number, got {}",
                args[0].type_name()
            ),
        ),
    )
}

fn prim_ts_from_epoch_millis(args: &[Value]) -> (SignalBits, Value) {
    let ms = match require_int(&args[0], "timestamp/from-epoch-millis") {
        Ok(n) => n,
        Err(e) => return e,
    };
    match jiff::Timestamp::from_millisecond(ms) {
        Ok(ts) => (SIG_OK, jiff_val(JiffValue::Timestamp(ts))),
        Err(e) => jiff_err("timestamp/from-epoch-millis", e),
    }
}

fn prim_ts_from_epoch_micros(args: &[Value]) -> (SignalBits, Value) {
    let us = match require_int(&args[0], "timestamp/from-epoch-micros") {
        Ok(n) => n,
        Err(e) => return e,
    };
    match jiff::Timestamp::from_microsecond(us) {
        Ok(ts) => (SIG_OK, jiff_val(JiffValue::Timestamp(ts))),
        Err(e) => jiff_err("timestamp/from-epoch-micros", e),
    }
}

fn prim_ts_from_epoch_nanos(args: &[Value]) -> (SignalBits, Value) {
    let ns = match require_int(&args[0], "timestamp/from-epoch-nanos") {
        Ok(n) => n as i128,
        Err(e) => return e,
    };
    match jiff::Timestamp::from_nanosecond(ns) {
        Ok(ts) => (SIG_OK, jiff_val(JiffValue::Timestamp(ts))),
        Err(e) => jiff_err("timestamp/from-epoch-nanos", e),
    }
}

pub static PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "temporal/string",
        func: prim_temporal_string,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Convert any temporal value to its ISO 8601 string representation.",
        params: &["val"],
        category: "jiff",
        example: "(temporal/string (date 2024 6 19))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "temporal/format",
        func: prim_temporal_format,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Format a temporal value using a strftime-style format string.",
        params: &["format", "val"],
        category: "jiff",
        example: r#"(temporal/format "%B %d, %Y" (date 2024 6 19))"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "timestamp/->epoch",
        func: prim_ts_epoch,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Timestamp as float seconds since Unix epoch.",
        params: &["ts"],
        category: "jiff",
        example: "(timestamp/->epoch (timestamp))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "timestamp/->epoch-millis",
        func: prim_ts_epoch_millis,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Timestamp as integer milliseconds since Unix epoch.",
        params: &["ts"],
        category: "jiff",
        example: "(timestamp/->epoch-millis (timestamp))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "timestamp/->epoch-micros",
        func: prim_ts_epoch_micros,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Timestamp as integer microseconds since Unix epoch.",
        params: &["ts"],
        category: "jiff",
        example: "(timestamp/->epoch-micros (timestamp))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "timestamp/->epoch-nanos",
        func: prim_ts_epoch_nanos,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Timestamp as integer nanoseconds since Unix epoch.",
        params: &["ts"],
        category: "jiff",
        example: "(timestamp/->epoch-nanos (timestamp))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "timestamp/from-epoch-seconds",
        func: prim_ts_from_epoch_seconds,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Construct a timestamp from seconds since Unix epoch (int or float).",
        params: &["seconds"],
        category: "jiff",
        example: "(timestamp/from-epoch-seconds 1718826165)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "timestamp/from-epoch-millis",
        func: prim_ts_from_epoch_millis,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Construct a timestamp from milliseconds since Unix epoch.",
        params: &["millis"],
        category: "jiff",
        example: "(timestamp/from-epoch-millis 1718826165000)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "timestamp/from-epoch-micros",
        func: prim_ts_from_epoch_micros,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Construct a timestamp from microseconds since Unix epoch.",
        params: &["micros"],
        category: "jiff",
        example: "(timestamp/from-epoch-micros 1718826165000000)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "timestamp/from-epoch-nanos",
        func: prim_ts_from_epoch_nanos,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Construct a timestamp from nanoseconds since Unix epoch.",
        params: &["nanos"],
        category: "jiff",
        example: "(timestamp/from-epoch-nanos 1718826165000000000)",
        aliases: &[],
    },
];
