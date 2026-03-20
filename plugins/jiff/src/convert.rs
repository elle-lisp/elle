//! Type conversions: between temporal types, zoned/in-tz, span->signed-duration.
//! Rounding: temporal/round.

use crate::{
    as_jiff, jiff_err, jiff_val, require_jiff, require_string, require_variant, struct_get_kw,
    JiffValue,
};
use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::Arity;
use elle::value::{error_val, Value};

// ---------------------------------------------------------------------------
// Conversion
// ---------------------------------------------------------------------------

/// (date/->date val) → extract date from datetime or zoned
fn prim_to_date(args: &[Value]) -> (SignalBits, Value) {
    match as_jiff(&args[0]) {
        Some(JiffValue::Date(d)) => (SIG_OK, jiff_val(JiffValue::Date(*d))),
        Some(JiffValue::DateTime(dt)) => (SIG_OK, jiff_val(JiffValue::Date(dt.date()))),
        Some(JiffValue::Zoned(z)) => (SIG_OK, jiff_val(JiffValue::Date(z.date()))),
        Some(other) => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "date/->date: expected date, datetime, or zoned, got {}",
                    other.type_name()
                ),
            ),
        ),
        None => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "date/->date: expected temporal value, got {}",
                    args[0].type_name()
                ),
            ),
        ),
    }
}

/// (time/->time val) → extract time from datetime or zoned
fn prim_to_time(args: &[Value]) -> (SignalBits, Value) {
    match as_jiff(&args[0]) {
        Some(JiffValue::Time(t)) => (SIG_OK, jiff_val(JiffValue::Time(*t))),
        Some(JiffValue::DateTime(dt)) => (SIG_OK, jiff_val(JiffValue::Time(dt.time()))),
        Some(JiffValue::Zoned(z)) => (SIG_OK, jiff_val(JiffValue::Time(z.time()))),
        Some(other) => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "time/->time: expected time, datetime, or zoned, got {}",
                    other.type_name()
                ),
            ),
        ),
        None => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "time/->time: expected temporal value, got {}",
                    args[0].type_name()
                ),
            ),
        ),
    }
}

/// (datetime/->datetime val) or (datetime/->datetime date time) → datetime
fn prim_to_datetime(args: &[Value]) -> (SignalBits, Value) {
    if args.len() == 2 {
        let d = match require_variant!(&args[0], Date, "datetime/->datetime", "date") {
            Ok(d) => *d,
            Err(e) => return e,
        };
        let t = match require_variant!(&args[1], Time, "datetime/->datetime", "time") {
            Ok(t) => *t,
            Err(e) => return e,
        };
        return (SIG_OK, jiff_val(JiffValue::DateTime(d.to_datetime(t))));
    }
    match as_jiff(&args[0]) {
        Some(JiffValue::DateTime(dt)) => (SIG_OK, jiff_val(JiffValue::DateTime(*dt))),
        Some(JiffValue::Zoned(z)) => (SIG_OK, jiff_val(JiffValue::DateTime(z.datetime()))),
        Some(JiffValue::Date(d)) => (
            SIG_OK,
            jiff_val(JiffValue::DateTime(
                d.to_datetime(jiff::civil::Time::midnight()),
            )),
        ),
        Some(other) => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "datetime/->datetime: expected datetime, zoned, or date, got {}",
                    other.type_name()
                ),
            ),
        ),
        None => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "datetime/->datetime: expected temporal value, got {}",
                    args[0].type_name()
                ),
            ),
        ),
    }
}

/// (timestamp/->timestamp val) → extract timestamp from zoned
fn prim_to_timestamp(args: &[Value]) -> (SignalBits, Value) {
    match as_jiff(&args[0]) {
        Some(JiffValue::Timestamp(ts)) => (SIG_OK, jiff_val(JiffValue::Timestamp(*ts))),
        Some(JiffValue::Zoned(z)) => (SIG_OK, jiff_val(JiffValue::Timestamp(z.timestamp()))),
        Some(other) => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "timestamp/->timestamp: expected timestamp or zoned, got {}",
                    other.type_name()
                ),
            ),
        ),
        None => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "timestamp/->timestamp: expected temporal value, got {}",
                    args[0].type_name()
                ),
            ),
        ),
    }
}

/// (zoned/in-tz val tz-string) → same instant, different timezone
fn prim_zoned_in_tz(args: &[Value]) -> (SignalBits, Value) {
    let z = match require_variant!(&args[0], Zoned, "zoned/in-tz", "zoned") {
        Ok(z) => z,
        Err(e) => return e,
    };
    let tz_str = match require_string(&args[1], "zoned/in-tz") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let tz = match jiff::tz::TimeZone::get(&tz_str) {
        Ok(tz) => tz,
        Err(e) => return jiff_err("zoned/in-tz", e),
    };
    let result = z.with_time_zone(tz);
    (SIG_OK, jiff_val(JiffValue::Zoned(Box::new(result))))
}

/// (span/->signed-duration span) → signed-duration (only for non-calendar spans)
fn prim_span_to_sd(args: &[Value]) -> (SignalBits, Value) {
    let s = match require_variant!(&args[0], Span, "span/->signed-duration", "span") {
        Ok(s) => *s,
        Err(e) => return e,
    };
    match jiff::SignedDuration::try_from(s) {
        Ok(d) => (SIG_OK, jiff_val(JiffValue::SignedDuration(d))),
        Err(e) => jiff_err("span/->signed-duration", e),
    }
}

// ---------------------------------------------------------------------------
// Rounding
// ---------------------------------------------------------------------------

fn parse_unit(s: &str) -> Option<jiff::Unit> {
    match s {
        "year" | "years" => Some(jiff::Unit::Year),
        "month" | "months" => Some(jiff::Unit::Month),
        "week" | "weeks" => Some(jiff::Unit::Week),
        "day" | "days" => Some(jiff::Unit::Day),
        "hour" | "hours" => Some(jiff::Unit::Hour),
        "minute" | "minutes" => Some(jiff::Unit::Minute),
        "second" | "seconds" => Some(jiff::Unit::Second),
        "millisecond" | "milliseconds" => Some(jiff::Unit::Millisecond),
        "microsecond" | "microseconds" => Some(jiff::Unit::Microsecond),
        "nanosecond" | "nanoseconds" => Some(jiff::Unit::Nanosecond),
        _ => None,
    }
}

/// (temporal/round val opts) → same type, rounded
///
/// opts is a struct: {:unit :hour} or {:unit :hour :mode :floor}
/// Supported modes: :ceil, :floor, :half-ceil, :half-floor, :half-even, :half-expand (default)
fn prim_temporal_round(args: &[Value]) -> (SignalBits, Value) {
    let jv = match require_jiff(&args[0], "temporal/round") {
        Ok(jv) => jv.clone(),
        Err(e) => return e,
    };
    let opts = &args[1];

    let unit_val = match struct_get_kw(opts, "unit") {
        Some(v) => v,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "jiff-error",
                    "temporal/round: opts must contain :unit keyword",
                ),
            )
        }
    };
    let unit_name = match unit_val.as_keyword_name() {
        Some(k) => k.to_string(),
        None => {
            return (
                SIG_ERROR,
                error_val("type-error", "temporal/round: :unit must be a keyword"),
            )
        }
    };
    let unit = match parse_unit(&unit_name) {
        Some(u) => u,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "jiff-error",
                    format!("temporal/round: unknown unit {:?}", unit_name),
                ),
            )
        }
    };

    // For now, just use the unit for rounding (mode support can be added later)
    match jv {
        JiffValue::Timestamp(ts) => match ts.round(unit) {
            Ok(r) => (SIG_OK, jiff_val(JiffValue::Timestamp(r))),
            Err(e) => jiff_err("temporal/round", e),
        },
        JiffValue::Time(t) => match t.round(unit) {
            Ok(r) => (SIG_OK, jiff_val(JiffValue::Time(r))),
            Err(e) => jiff_err("temporal/round", e),
        },
        JiffValue::DateTime(dt) => match dt.round(unit) {
            Ok(r) => (SIG_OK, jiff_val(JiffValue::DateTime(r))),
            Err(e) => jiff_err("temporal/round", e),
        },
        JiffValue::Zoned(z) => match z.round(unit) {
            Ok(r) => (SIG_OK, jiff_val(JiffValue::Zoned(Box::new(r)))),
            Err(e) => jiff_err("temporal/round", e),
        },
        other => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("temporal/round: cannot round {}", other.type_name()),
            ),
        ),
    }
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

pub static PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "date/->date",
        func: prim_to_date,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Extract or convert to a date. Works on date, datetime, zoned.",
        params: &["val"],
        category: "jiff",
        example: "(date/->date (now))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "time/->time",
        func: prim_to_time,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Extract or convert to a time. Works on time, datetime, zoned.",
        params: &["val"],
        category: "jiff",
        example: "(time/->time (now))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "datetime/->datetime",
        func: prim_to_datetime,
        signal: Signal::errors(),
        arity: Arity::Range(1, 2),
        doc: "Extract datetime from zoned/date, or combine a date and time.",
        params: &["val-or-date", "time?"],
        category: "jiff",
        example: "(datetime/->datetime (now))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "timestamp/->timestamp",
        func: prim_to_timestamp,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Extract timestamp from zoned, or identity on timestamp.",
        params: &["val"],
        category: "jiff",
        example: "(timestamp/->timestamp (now))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "zoned/in-tz",
        func: prim_zoned_in_tz,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Convert a zoned datetime to a different timezone (same instant).",
        params: &["zoned", "tz"],
        category: "jiff",
        example: r#"(zoned/in-tz (now) "America/Los_Angeles")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "span/->signed-duration",
        func: prim_span_to_sd,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Convert a span to a signed-duration. Fails if span has calendar units (years, months, weeks).",
        params: &["span"],
        category: "jiff",
        example: "(span/->signed-duration (span {:hours 1 :minutes 30}))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "temporal/round",
        func: prim_temporal_round,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Round a temporal value. Opts struct must contain :unit keyword.",
        params: &["val", "opts"],
        category: "jiff",
        example: "(temporal/round (time 15 22 45) {:unit :hour})",
        aliases: &[],
    },
];
