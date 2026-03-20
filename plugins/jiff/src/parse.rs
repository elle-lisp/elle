//! Parsing: date/parse, time/parse, datetime/parse, timestamp/parse,
//! zoned/parse, span/parse, signed-duration/parse, temporal/parse-with.

use crate::{jiff_err, jiff_val, require_string, JiffValue};
use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::fiber::{SignalBits, SIG_OK};
use elle::value::types::Arity;
use elle::value::Value;

fn prim_date_parse(args: &[Value]) -> (SignalBits, Value) {
    let s = match require_string(&args[0], "date/parse") {
        Ok(s) => s,
        Err(e) => return e,
    };
    match s.parse::<jiff::civil::Date>() {
        Ok(d) => (SIG_OK, jiff_val(JiffValue::Date(d))),
        Err(e) => jiff_err("date/parse", e),
    }
}

fn prim_time_parse(args: &[Value]) -> (SignalBits, Value) {
    let s = match require_string(&args[0], "time/parse") {
        Ok(s) => s,
        Err(e) => return e,
    };
    match s.parse::<jiff::civil::Time>() {
        Ok(t) => (SIG_OK, jiff_val(JiffValue::Time(t))),
        Err(e) => jiff_err("time/parse", e),
    }
}

fn prim_datetime_parse(args: &[Value]) -> (SignalBits, Value) {
    let s = match require_string(&args[0], "datetime/parse") {
        Ok(s) => s,
        Err(e) => return e,
    };
    match s.parse::<jiff::civil::DateTime>() {
        Ok(dt) => (SIG_OK, jiff_val(JiffValue::DateTime(dt))),
        Err(e) => jiff_err("datetime/parse", e),
    }
}

fn prim_timestamp_parse(args: &[Value]) -> (SignalBits, Value) {
    let s = match require_string(&args[0], "timestamp/parse") {
        Ok(s) => s,
        Err(e) => return e,
    };
    match s.parse::<jiff::Timestamp>() {
        Ok(ts) => (SIG_OK, jiff_val(JiffValue::Timestamp(ts))),
        Err(e) => jiff_err("timestamp/parse", e),
    }
}

fn prim_zoned_parse(args: &[Value]) -> (SignalBits, Value) {
    let s = match require_string(&args[0], "zoned/parse") {
        Ok(s) => s,
        Err(e) => return e,
    };
    match s.parse::<jiff::Zoned>() {
        Ok(z) => (SIG_OK, jiff_val(JiffValue::Zoned(Box::new(z)))),
        Err(e) => jiff_err("zoned/parse", e),
    }
}

fn prim_span_parse(args: &[Value]) -> (SignalBits, Value) {
    let s = match require_string(&args[0], "span/parse") {
        Ok(s) => s,
        Err(e) => return e,
    };
    match s.parse::<jiff::Span>() {
        Ok(sp) => (SIG_OK, jiff_val(JiffValue::Span(sp))),
        Err(e) => jiff_err("span/parse", e),
    }
}

fn prim_signed_duration_parse(args: &[Value]) -> (SignalBits, Value) {
    let s = match require_string(&args[0], "signed-duration/parse") {
        Ok(s) => s,
        Err(e) => return e,
    };
    match s.parse::<jiff::SignedDuration>() {
        Ok(d) => (SIG_OK, jiff_val(JiffValue::SignedDuration(d))),
        Err(e) => jiff_err("signed-duration/parse", e),
    }
}

/// (temporal/parse-with fmt string) → datetime or zoned
///
/// Uses strftime-style format to parse a string.  Returns zoned if the
/// format includes timezone info, datetime otherwise.
fn prim_temporal_parse_with(args: &[Value]) -> (SignalBits, Value) {
    let fmt = match require_string(&args[0], "temporal/parse-with") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let input = match require_string(&args[1], "temporal/parse-with") {
        Ok(s) => s,
        Err(e) => return e,
    };
    // Try zoned first (has timezone), fall back to datetime
    let parser = jiff::fmt::strtime::BrokenDownTime::parse(&fmt, &input);
    match parser {
        Ok(bdt) => {
            // Try to produce a zoned
            if let Ok(z) = bdt.to_zoned() {
                return (SIG_OK, jiff_val(JiffValue::Zoned(Box::new(z))));
            }
            // Fall back to datetime
            match bdt.to_datetime() {
                Ok(dt) => (SIG_OK, jiff_val(JiffValue::DateTime(dt))),
                Err(e) => jiff_err("temporal/parse-with", e),
            }
        }
        Err(e) => jiff_err("temporal/parse-with", e),
    }
}

pub static PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "date/parse",
        func: prim_date_parse,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Parse an ISO 8601 date string.",
        params: &["string"],
        category: "jiff",
        example: r#"(date/parse "2024-06-19")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "time/parse",
        func: prim_time_parse,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Parse an ISO 8601 time string.",
        params: &["string"],
        category: "jiff",
        example: r#"(time/parse "15:22:45")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "datetime/parse",
        func: prim_datetime_parse,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Parse an ISO 8601 datetime string.",
        params: &["string"],
        category: "jiff",
        example: r#"(datetime/parse "2024-06-19T15:22:45")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "timestamp/parse",
        func: prim_timestamp_parse,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Parse an ISO 8601 timestamp string (must include UTC offset).",
        params: &["string"],
        category: "jiff",
        example: r#"(timestamp/parse "2024-06-19T19:22:45Z")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "zoned/parse",
        func: prim_zoned_parse,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Parse an ISO 8601 zoned datetime string (includes timezone annotation).",
        params: &["string"],
        category: "jiff",
        example: r#"(zoned/parse "2024-06-19T15:22:45-04:00[America/New_York]")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "span/parse",
        func: prim_span_parse,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Parse an ISO 8601 duration string.",
        params: &["string"],
        category: "jiff",
        example: r#"(span/parse "P1Y2M3DT4H5M6S")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "signed-duration/parse",
        func: prim_signed_duration_parse,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Parse an ISO 8601 duration string as an exact signed duration.",
        params: &["string"],
        category: "jiff",
        example: r#"(signed-duration/parse "PT3600S")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "temporal/parse-with",
        func: prim_temporal_parse_with,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Parse a string using a strftime-style format. Returns zoned if format includes timezone, datetime otherwise.",
        params: &["format", "string"],
        category: "jiff",
        example: r#"(temporal/parse-with "%Y-%m-%d" "2024-06-19")"#,
        aliases: &[],
    },
];
