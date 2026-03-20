//! Arithmetic: add/sub for all temporal types, since/until, span ops.
//! Comparison: temporal/compare, temporal/before?, temporal/after?, temporal/equal?

use crate::{as_jiff, jiff_err, jiff_val, require_int, require_jiff, require_keyword, require_variant, JiffValue};
use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::Arity;
use elle::value::{error_val, Value};
use jiff::Unit;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract a Span or SignedDuration for arithmetic.
fn require_span_like<'a>(v: &'a Value, fn_name: &str) -> Result<&'a JiffValue, (SignalBits, Value)> {
    match as_jiff(v) {
        Some(jv @ JiffValue::Span(_)) | Some(jv @ JiffValue::SignedDuration(_)) => Ok(jv),
        Some(other) => Err((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: expected span or signed-duration, got {}",
                    fn_name,
                    other.type_name()
                ),
            ),
        )),
        None => Err((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: expected span or signed-duration, got {}",
                    fn_name,
                    v.type_name()
                ),
            ),
        )),
    }
}

fn parse_unit(s: &str) -> Option<Unit> {
    match s {
        "year" | "years" => Some(Unit::Year),
        "month" | "months" => Some(Unit::Month),
        "week" | "weeks" => Some(Unit::Week),
        "day" | "days" => Some(Unit::Day),
        "hour" | "hours" => Some(Unit::Hour),
        "minute" | "minutes" => Some(Unit::Minute),
        "second" | "seconds" => Some(Unit::Second),
        "millisecond" | "milliseconds" => Some(Unit::Millisecond),
        "microsecond" | "microseconds" => Some(Unit::Microsecond),
        "nanosecond" | "nanoseconds" => Some(Unit::Nanosecond),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Add / Sub  (type/add val span-or-duration)
// ---------------------------------------------------------------------------

macro_rules! arith_prim {
    ($fn_name:ident, $prim_name:expr, $variant:ident, $type_name:expr, $op:ident) => {
        fn $fn_name(args: &[Value]) -> (SignalBits, Value) {
            let v = match require_variant!(&args[0], $variant, $prim_name, $type_name) {
                Ok(v) => v.clone(),
                Err(e) => return e,
            };
            let rhs = match require_span_like(&args[1], $prim_name) {
                Ok(jv) => jv,
                Err(e) => return e,
            };
            match rhs {
                JiffValue::Span(s) => match v.$op(*s) {
                    Ok(r) => (SIG_OK, jiff_val(JiffValue::$variant(r))),
                    Err(e) => jiff_err($prim_name, e),
                },
                JiffValue::SignedDuration(d) => match v.$op(*d) {
                    Ok(r) => (SIG_OK, jiff_val(JiffValue::$variant(r))),
                    Err(e) => jiff_err($prim_name, e),
                },
                _ => unreachable!(),
            }
        }
    };
}

arith_prim!(prim_date_add, "date/add", Date, "date", checked_add);
arith_prim!(prim_date_sub, "date/sub", Date, "date", checked_sub);
arith_prim!(prim_time_add, "time/add", Time, "time", checked_add);
arith_prim!(prim_time_sub, "time/sub", Time, "time", checked_sub);
arith_prim!(prim_datetime_add, "datetime/add", DateTime, "datetime", checked_add);
arith_prim!(prim_datetime_sub, "datetime/sub", DateTime, "datetime", checked_sub);
arith_prim!(prim_timestamp_add, "timestamp/add", Timestamp, "timestamp", checked_add);
arith_prim!(prim_timestamp_sub, "timestamp/sub", Timestamp, "timestamp", checked_sub);

// Zoned is boxed, needs special handling
fn prim_zoned_add(args: &[Value]) -> (SignalBits, Value) {
    let z = match require_variant!(&args[0], Zoned, "zoned/add", "zoned") {
        Ok(z) => z,
        Err(e) => return e,
    };
    let rhs = match require_span_like(&args[1], "zoned/add") {
        Ok(jv) => jv,
        Err(e) => return e,
    };
    match rhs {
        JiffValue::Span(s) => match z.as_ref().checked_add(*s) {
            Ok(r) => (SIG_OK, jiff_val(JiffValue::Zoned(Box::new(r)))),
            Err(e) => jiff_err("zoned/add", e),
        },
        JiffValue::SignedDuration(d) => match z.as_ref().checked_add(*d) {
            Ok(r) => (SIG_OK, jiff_val(JiffValue::Zoned(Box::new(r)))),
            Err(e) => jiff_err("zoned/add", e),
        },
        _ => unreachable!(),
    }
}

fn prim_zoned_sub(args: &[Value]) -> (SignalBits, Value) {
    let z = match require_variant!(&args[0], Zoned, "zoned/sub", "zoned") {
        Ok(z) => z,
        Err(e) => return e,
    };
    let rhs = match require_span_like(&args[1], "zoned/sub") {
        Ok(jv) => jv,
        Err(e) => return e,
    };
    match rhs {
        JiffValue::Span(s) => match z.as_ref().checked_sub(*s) {
            Ok(r) => (SIG_OK, jiff_val(JiffValue::Zoned(Box::new(r)))),
            Err(e) => jiff_err("zoned/sub", e),
        },
        JiffValue::SignedDuration(d) => match z.as_ref().checked_sub(*d) {
            Ok(r) => (SIG_OK, jiff_val(JiffValue::Zoned(Box::new(r)))),
            Err(e) => jiff_err("zoned/sub", e),
        },
        _ => unreachable!(),
    }
}

// ---------------------------------------------------------------------------
// Since / Until
// ---------------------------------------------------------------------------

/// (timestamp/since a b) or (timestamp/since a b unit) → signed-duration
fn prim_timestamp_since(args: &[Value]) -> (SignalBits, Value) {
    let a = match require_variant!(&args[0], Timestamp, "timestamp/since", "timestamp") {
        Ok(ts) => *ts,
        Err(e) => return e,
    };
    let b = match require_variant!(&args[1], Timestamp, "timestamp/since", "timestamp") {
        Ok(ts) => *ts,
        Err(e) => return e,
    };
    if args.len() > 2 {
        let unit_kw = match require_keyword(&args[2], "timestamp/since") {
            Ok(k) => k,
            Err(e) => return e,
        };
        let unit = match parse_unit(&unit_kw) {
            Some(u) => u,
            None => return (SIG_ERROR, error_val("jiff-error", format!("timestamp/since: unknown unit {:?}", unit_kw))),
        };
        match a.since((unit, b)) {
            Ok(s) => (SIG_OK, jiff_val(JiffValue::Span(s))),
            Err(e) => jiff_err("timestamp/since", e),
        }
    } else {
        let d = a.duration_since(b);
        (SIG_OK, jiff_val(JiffValue::SignedDuration(d)))
    }
}

/// (timestamp/until a b) or (timestamp/until a b unit) → signed-duration or span
fn prim_timestamp_until(args: &[Value]) -> (SignalBits, Value) {
    let a = match require_variant!(&args[0], Timestamp, "timestamp/until", "timestamp") {
        Ok(ts) => *ts,
        Err(e) => return e,
    };
    let b = match require_variant!(&args[1], Timestamp, "timestamp/until", "timestamp") {
        Ok(ts) => *ts,
        Err(e) => return e,
    };
    if args.len() > 2 {
        let unit_kw = match require_keyword(&args[2], "timestamp/until") {
            Ok(k) => k,
            Err(e) => return e,
        };
        let unit = match parse_unit(&unit_kw) {
            Some(u) => u,
            None => return (SIG_ERROR, error_val("jiff-error", format!("timestamp/until: unknown unit {:?}", unit_kw))),
        };
        match a.until((unit, b)) {
            Ok(s) => (SIG_OK, jiff_val(JiffValue::Span(s))),
            Err(e) => jiff_err("timestamp/until", e),
        }
    } else {
        let d = a.duration_until(b);
        (SIG_OK, jiff_val(JiffValue::SignedDuration(d)))
    }
}

/// (zoned/until a b) or (zoned/until a b unit) → span
fn prim_zoned_until(args: &[Value]) -> (SignalBits, Value) {
    let a = match require_variant!(&args[0], Zoned, "zoned/until", "zoned") {
        Ok(z) => z,
        Err(e) => return e,
    };
    let b = match require_variant!(&args[1], Zoned, "zoned/until", "zoned") {
        Ok(z) => z,
        Err(e) => return e,
    };
    if args.len() > 2 {
        let unit_kw = match require_keyword(&args[2], "zoned/until") {
            Ok(k) => k,
            Err(e) => return e,
        };
        let unit = match parse_unit(&unit_kw) {
            Some(u) => u,
            None => return (SIG_ERROR, error_val("jiff-error", format!("zoned/until: unknown unit {:?}", unit_kw))),
        };
        match a.as_ref().until((unit, b.as_ref())) {
            Ok(s) => (SIG_OK, jiff_val(JiffValue::Span(s))),
            Err(e) => jiff_err("zoned/until", e),
        }
    } else {
        match a.as_ref().until(b.as_ref()) {
            Ok(s) => (SIG_OK, jiff_val(JiffValue::Span(s))),
            Err(e) => jiff_err("zoned/until", e),
        }
    }
}

// ---------------------------------------------------------------------------
// Span arithmetic
// ---------------------------------------------------------------------------

fn prim_span_add(args: &[Value]) -> (SignalBits, Value) {
    let a = match require_variant!(&args[0], Span, "span/add", "span") {
        Ok(s) => *s,
        Err(e) => return e,
    };
    let b = match require_variant!(&args[1], Span, "span/add", "span") {
        Ok(s) => *s,
        Err(e) => return e,
    };
    match a.checked_add(b) {
        Ok(s) => (SIG_OK, jiff_val(JiffValue::Span(s))),
        Err(e) => jiff_err("span/add", e),
    }
}

fn prim_span_mul(args: &[Value]) -> (SignalBits, Value) {
    let s = match require_variant!(&args[0], Span, "span/mul", "span") {
        Ok(s) => *s,
        Err(e) => return e,
    };
    let n = match require_int(&args[1], "span/mul") {
        Ok(n) => n,
        Err(e) => return e,
    };
    match s.checked_mul(n) {
        Ok(r) => (SIG_OK, jiff_val(JiffValue::Span(r))),
        Err(e) => jiff_err("span/mul", e),
    }
}

fn prim_span_negate(args: &[Value]) -> (SignalBits, Value) {
    let s = match require_variant!(&args[0], Span, "span/negate", "span") {
        Ok(s) => *s,
        Err(e) => return e,
    };
    (SIG_OK, jiff_val(JiffValue::Span(s.negate())))
}

fn prim_span_abs(args: &[Value]) -> (SignalBits, Value) {
    let s = match require_variant!(&args[0], Span, "span/abs", "span") {
        Ok(s) => *s,
        Err(e) => return e,
    };
    (SIG_OK, jiff_val(JiffValue::Span(s.abs())))
}

fn prim_span_total(args: &[Value]) -> (SignalBits, Value) {
    let s = match require_variant!(&args[0], Span, "span-total", "span") {
        Ok(s) => *s,
        Err(e) => return e,
    };
    let unit_kw = match require_keyword(&args[1], "span-total") {
        Ok(k) => k,
        Err(e) => return e,
    };
    let unit = match parse_unit(&unit_kw) {
        Some(u) => u,
        None => {
            return (
                SIG_ERROR,
                error_val("jiff-error", format!("span-total: unknown unit {:?}", unit_kw)),
            )
        }
    };
    match s.total(unit) {
        Ok(f) => (SIG_OK, Value::float(f)),
        Err(e) => jiff_err("span-total", e),
    }
}

// ---------------------------------------------------------------------------
// SignedDuration arithmetic
// ---------------------------------------------------------------------------

fn prim_sd_add(args: &[Value]) -> (SignalBits, Value) {
    let a = match require_variant!(&args[0], SignedDuration, "signed-duration/add", "signed-duration") {
        Ok(d) => *d,
        Err(e) => return e,
    };
    let b = match require_variant!(&args[1], SignedDuration, "signed-duration/add", "signed-duration") {
        Ok(d) => *d,
        Err(e) => return e,
    };
    match a.checked_add(b) {
        Some(r) => (SIG_OK, jiff_val(JiffValue::SignedDuration(r))),
        None => (SIG_ERROR, error_val("jiff-error", "signed-duration/add: overflow")),
    }
}

fn prim_sd_negate(args: &[Value]) -> (SignalBits, Value) {
    let d = match require_variant!(&args[0], SignedDuration, "signed-duration/negate", "signed-duration") {
        Ok(d) => *d,
        Err(e) => return e,
    };
    match d.checked_neg() {
        Some(r) => (SIG_OK, jiff_val(JiffValue::SignedDuration(r))),
        None => (SIG_ERROR, error_val("jiff-error", "signed-duration/negate: overflow")),
    }
}

fn prim_sd_abs(args: &[Value]) -> (SignalBits, Value) {
    let d = match require_variant!(&args[0], SignedDuration, "signed-duration/abs", "signed-duration") {
        Ok(d) => *d,
        Err(e) => return e,
    };
    (SIG_OK, jiff_val(JiffValue::SignedDuration(d.abs())))
}

// ---------------------------------------------------------------------------
// Comparison (polymorphic)
// ---------------------------------------------------------------------------

/// Compare two temporal values of the same type.  Returns -1, 0, or 1.
fn prim_temporal_compare(args: &[Value]) -> (SignalBits, Value) {
    let a = match require_jiff(&args[0], "temporal/compare") {
        Ok(jv) => jv,
        Err(e) => return e,
    };
    let b = match require_jiff(&args[1], "temporal/compare") {
        Ok(jv) => jv,
        Err(e) => return e,
    };
    use std::cmp::Ordering;
    let ord = match (a, b) {
        (JiffValue::Timestamp(a), JiffValue::Timestamp(b)) => a.cmp(b),
        (JiffValue::Date(a), JiffValue::Date(b)) => a.cmp(b),
        (JiffValue::Time(a), JiffValue::Time(b)) => a.cmp(b),
        (JiffValue::DateTime(a), JiffValue::DateTime(b)) => a.cmp(b),
        (JiffValue::Zoned(a), JiffValue::Zoned(b)) => a.timestamp().cmp(&b.timestamp()),
        (JiffValue::SignedDuration(a), JiffValue::SignedDuration(b)) => a.cmp(b),
        (JiffValue::Span(a), JiffValue::Span(b)) => {
            // Fieldwise comparison
            let fields_a = span_fields(a);
            let fields_b = span_fields(b);
            fields_a.cmp(&fields_b)
        }
        _ => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "temporal/compare: cannot compare {} with {}",
                        a.type_name(),
                        b.type_name()
                    ),
                ),
            )
        }
    };
    let n = match ord {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    };
    (SIG_OK, Value::int(n))
}

fn span_fields(s: &jiff::Span) -> [i64; 10] {
    [
        s.get_years() as i64,
        s.get_months() as i64,
        s.get_weeks() as i64,
        s.get_days() as i64,
        s.get_hours() as i64,
        s.get_minutes() as i64,
        s.get_seconds() as i64,
        s.get_milliseconds() as i64,
        s.get_microseconds() as i64,
        s.get_nanoseconds() as i64,
    ]
}

fn prim_temporal_before(args: &[Value]) -> (SignalBits, Value) {
    let (sig, val) = prim_temporal_compare(args);
    if sig != SIG_OK {
        return (sig, val);
    }
    (SIG_OK, Value::bool(val.as_int() == Some(-1)))
}

fn prim_temporal_after(args: &[Value]) -> (SignalBits, Value) {
    let (sig, val) = prim_temporal_compare(args);
    if sig != SIG_OK {
        return (sig, val);
    }
    (SIG_OK, Value::bool(val.as_int() == Some(1)))
}

fn prim_temporal_equal(args: &[Value]) -> (SignalBits, Value) {
    let (sig, val) = prim_temporal_compare(args);
    if sig != SIG_OK {
        return (sig, val);
    }
    (SIG_OK, Value::bool(val.as_int() == Some(0)))
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

pub static PRIMITIVES: &[PrimitiveDef] = &[
    // Add
    PrimitiveDef {
        name: "date/add",
        func: prim_date_add,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Add a span or signed-duration to a date.",
        params: &["date", "span"],
        category: "jiff",
        example: "(date/add (date 2024 6 19) (span {:days 30}))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "date/sub",
        func: prim_date_sub,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Subtract a span or signed-duration from a date.",
        params: &["date", "span"],
        category: "jiff",
        example: "(date/sub (date 2024 6 19) (span {:days 30}))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "time/add",
        func: prim_time_add,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Add a span or signed-duration to a time.",
        params: &["time", "span"],
        category: "jiff",
        example: "(time/add (time 15 22 45) (span {:hours 2}))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "time/sub",
        func: prim_time_sub,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Subtract a span or signed-duration from a time.",
        params: &["time", "span"],
        category: "jiff",
        example: "(time/sub (time 15 22 45) (span {:hours 2}))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "datetime/add",
        func: prim_datetime_add,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Add a span or signed-duration to a datetime.",
        params: &["datetime", "span"],
        category: "jiff",
        example: "(datetime/add (datetime 2024 6 19 15 22 45) (span {:hours 2}))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "datetime/sub",
        func: prim_datetime_sub,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Subtract a span or signed-duration from a datetime.",
        params: &["datetime", "span"],
        category: "jiff",
        example: "(datetime/sub (datetime 2024 6 19 15 22 45) (span {:hours 2}))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "timestamp/add",
        func: prim_timestamp_add,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Add a span or signed-duration to a timestamp.",
        params: &["timestamp", "span"],
        category: "jiff",
        example: "(timestamp/add (timestamp) (span {:hours 2}))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "timestamp/sub",
        func: prim_timestamp_sub,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Subtract a span or signed-duration from a timestamp.",
        params: &["timestamp", "span"],
        category: "jiff",
        example: "(timestamp/sub (timestamp) (span {:hours 2}))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "zoned/add",
        func: prim_zoned_add,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Add a span or signed-duration to a zoned datetime.",
        params: &["zoned", "span"],
        category: "jiff",
        example: r#"(zoned/add (now) (span {:hours 2}))"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "zoned/sub",
        func: prim_zoned_sub,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Subtract a span or signed-duration from a zoned datetime.",
        params: &["zoned", "span"],
        category: "jiff",
        example: r#"(zoned/sub (now) (span {:hours 2}))"#,
        aliases: &[],
    },
    // Since / Until
    PrimitiveDef {
        name: "timestamp/since",
        func: prim_timestamp_since,
        signal: Signal::errors(),
        arity: Arity::Range(2, 3),
        doc: "Signed duration from b to a. Optional unit keyword for span result.",
        params: &["a", "b", "unit?"],
        category: "jiff",
        example: "(timestamp/since ts1 ts2)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "timestamp/until",
        func: prim_timestamp_until,
        signal: Signal::errors(),
        arity: Arity::Range(2, 3),
        doc: "Signed duration from a to b. Optional unit keyword for span result.",
        params: &["a", "b", "unit?"],
        category: "jiff",
        example: "(timestamp/until ts1 ts2)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "zoned/until",
        func: prim_zoned_until,
        signal: Signal::errors(),
        arity: Arity::Range(2, 3),
        doc: "Span from a to b. Optional unit keyword for largest unit.",
        params: &["a", "b", "unit?"],
        category: "jiff",
        example: "(zoned/until z1 z2)",
        aliases: &[],
    },
    // Span arithmetic
    PrimitiveDef {
        name: "span/add",
        func: prim_span_add,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Add two spans.",
        params: &["a", "b"],
        category: "jiff",
        example: "(span/add (span {:hours 1}) (span {:minutes 30}))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "span/mul",
        func: prim_span_mul,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Multiply a span by an integer.",
        params: &["span", "n"],
        category: "jiff",
        example: "(span/mul (span {:hours 1}) 3)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "span/negate",
        func: prim_span_negate,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Negate a span (flip sign of all fields).",
        params: &["span"],
        category: "jiff",
        example: "(span/negate (span {:hours 1}))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "span/abs",
        func: prim_span_abs,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Absolute value of a span.",
        params: &["span"],
        category: "jiff",
        example: "(span/abs (span/negate (span {:hours 1})))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "span-total",
        func: prim_span_total,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Total value of a span in the given unit as a float. Only works for spans with no calendar units unless the unit is calendar.",
        params: &["span", "unit"],
        category: "jiff",
        example: "(span-total (span {:hours 1 :minutes 30}) :minutes)",
        aliases: &[],
    },
    // SignedDuration arithmetic
    PrimitiveDef {
        name: "signed-duration/add",
        func: prim_sd_add,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Add two signed-durations.",
        params: &["a", "b"],
        category: "jiff",
        example: "(signed-duration/add (signed-duration 3600) (signed-duration 1800))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "signed-duration/negate",
        func: prim_sd_negate,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Negate a signed-duration.",
        params: &["dur"],
        category: "jiff",
        example: "(signed-duration/negate (signed-duration 3600))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "signed-duration/abs",
        func: prim_sd_abs,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Absolute value of a signed-duration.",
        params: &["dur"],
        category: "jiff",
        example: "(signed-duration/abs (signed-duration -3600))",
        aliases: &[],
    },
    // Comparison
    PrimitiveDef {
        name: "temporal/compare",
        func: prim_temporal_compare,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Compare two temporal values of the same type. Returns -1, 0, or 1.",
        params: &["a", "b"],
        category: "jiff",
        example: "(temporal/compare (date 2024 1 1) (date 2024 6 19))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "temporal/before?",
        func: prim_temporal_before,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "True if a is before b.",
        params: &["a", "b"],
        category: "jiff",
        example: "(temporal/before? (date 2024 1 1) (date 2024 6 19))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "temporal/after?",
        func: prim_temporal_after,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "True if a is after b.",
        params: &["a", "b"],
        category: "jiff",
        example: "(temporal/after? (date 2024 6 19) (date 2024 1 1))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "temporal/equal?",
        func: prim_temporal_equal,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "True if a and b represent the same instant/value.",
        params: &["a", "b"],
        category: "jiff",
        example: "(temporal/equal? (date 2024 6 19) (date 2024 6 19))",
        aliases: &[],
    },
];
