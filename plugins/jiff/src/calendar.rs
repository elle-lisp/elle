//! Calendar helpers, timezone ops, and series generation.

use crate::{
    as_jiff, jiff_err, jiff_val, require_int, require_jiff, require_keyword, require_string,
    require_variant, JiffValue,
};
use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::Arity;
use elle::value::{error_val, Value};

// ---------------------------------------------------------------------------
// Calendar helpers — extract a civil::Date from date, datetime, or zoned
// ---------------------------------------------------------------------------

fn extract_date(v: &Value, fn_name: &str) -> Result<jiff::civil::Date, (SignalBits, Value)> {
    match as_jiff(v) {
        Some(JiffValue::Date(d)) => Ok(*d),
        Some(JiffValue::DateTime(dt)) => Ok(dt.date()),
        Some(JiffValue::Zoned(z)) => Ok(z.date()),
        Some(other) => Err((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: expected date, datetime, or zoned, got {}",
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
                    "{}: expected date, datetime, or zoned, got {}",
                    fn_name,
                    v.type_name()
                ),
            ),
        )),
    }
}

fn prim_date_start_of_month(args: &[Value]) -> (SignalBits, Value) {
    let d = match extract_date(&args[0], "date/start-of-month") {
        Ok(d) => d,
        Err(e) => return e,
    };
    (SIG_OK, jiff_val(JiffValue::Date(d.first_of_month())))
}

fn prim_date_end_of_month(args: &[Value]) -> (SignalBits, Value) {
    let d = match extract_date(&args[0], "date/end-of-month") {
        Ok(d) => d,
        Err(e) => return e,
    };
    (SIG_OK, jiff_val(JiffValue::Date(d.last_of_month())))
}

fn prim_date_start_of_year(args: &[Value]) -> (SignalBits, Value) {
    let d = match extract_date(&args[0], "date/start-of-year") {
        Ok(d) => d,
        Err(e) => return e,
    };
    match jiff::civil::Date::new(d.year(), 1, 1) {
        Ok(d) => (SIG_OK, jiff_val(JiffValue::Date(d))),
        Err(e) => jiff_err("date/start-of-year", e),
    }
}

fn prim_date_end_of_year(args: &[Value]) -> (SignalBits, Value) {
    let d = match extract_date(&args[0], "date/end-of-year") {
        Ok(d) => d,
        Err(e) => return e,
    };
    match jiff::civil::Date::new(d.year(), 12, 31) {
        Ok(d) => (SIG_OK, jiff_val(JiffValue::Date(d))),
        Err(e) => jiff_err("date/end-of-year", e),
    }
}

fn parse_weekday(s: &str) -> Option<jiff::civil::Weekday> {
    match s {
        "monday" => Some(jiff::civil::Weekday::Monday),
        "tuesday" => Some(jiff::civil::Weekday::Tuesday),
        "wednesday" => Some(jiff::civil::Weekday::Wednesday),
        "thursday" => Some(jiff::civil::Weekday::Thursday),
        "friday" => Some(jiff::civil::Weekday::Friday),
        "saturday" => Some(jiff::civil::Weekday::Saturday),
        "sunday" => Some(jiff::civil::Weekday::Sunday),
        _ => None,
    }
}

fn prim_date_next_weekday(args: &[Value]) -> (SignalBits, Value) {
    let d = match extract_date(&args[0], "date/next-weekday") {
        Ok(d) => d,
        Err(e) => return e,
    };
    let kw = match require_keyword(&args[1], "date/next-weekday") {
        Ok(k) => k,
        Err(e) => return e,
    };
    let wd = match parse_weekday(&kw) {
        Some(wd) => wd,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "jiff-error",
                    format!("date/next-weekday: unknown weekday {:?}", kw),
                ),
            )
        }
    };
    match d.nth_weekday(1, wd) {
        Ok(d) => (SIG_OK, jiff_val(JiffValue::Date(d))),
        Err(e) => jiff_err("date/next-weekday", e),
    }
}

fn prim_date_prev_weekday(args: &[Value]) -> (SignalBits, Value) {
    let d = match extract_date(&args[0], "date/prev-weekday") {
        Ok(d) => d,
        Err(e) => return e,
    };
    let kw = match require_keyword(&args[1], "date/prev-weekday") {
        Ok(k) => k,
        Err(e) => return e,
    };
    let wd = match parse_weekday(&kw) {
        Some(wd) => wd,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "jiff-error",
                    format!("date/prev-weekday: unknown weekday {:?}", kw),
                ),
            )
        }
    };
    match d.nth_weekday(-1, wd) {
        Ok(d) => (SIG_OK, jiff_val(JiffValue::Date(d))),
        Err(e) => jiff_err("date/prev-weekday", e),
    }
}

// ---------------------------------------------------------------------------
// Timezone operations
// ---------------------------------------------------------------------------

fn prim_tz_list(_args: &[Value]) -> (SignalBits, Value) {
    let names: Vec<Value> = jiff::tz::db()
        .available()
        .map(|name| Value::string(name.as_str()))
        .collect();
    (SIG_OK, elle::list(names))
}

fn prim_tz_valid(args: &[Value]) -> (SignalBits, Value) {
    let name = match require_string(&args[0], "tz-valid?") {
        Ok(s) => s,
        Err(e) => return e,
    };
    (SIG_OK, Value::bool(jiff::tz::TimeZone::get(&name).is_ok()))
}

fn prim_tz_system(_args: &[Value]) -> (SignalBits, Value) {
    match jiff::tz::TimeZone::system().iana_name() {
        Some(name) => (SIG_OK, Value::string(name)),
        None => (
            SIG_ERROR,
            error_val(
                "jiff-error",
                "tz-system: could not determine system timezone",
            ),
        ),
    }
}

fn prim_tz_fixed(args: &[Value]) -> (SignalBits, Value) {
    let offset_secs = match require_int(&args[0], "tz-fixed") {
        Ok(n) => n as i32,
        Err(e) => return e,
    };
    let offset = match jiff::tz::Offset::from_seconds(offset_secs) {
        Ok(o) => o,
        Err(e) => return jiff_err("tz-fixed", e),
    };
    let s = offset.to_string();
    (SIG_OK, Value::string(s.as_str()))
}

// ---------------------------------------------------------------------------
// Series generation
// ---------------------------------------------------------------------------

/// (temporal/series start span count) → list of temporal values
fn prim_temporal_series(args: &[Value]) -> (SignalBits, Value) {
    let start = match require_jiff(&args[0], "temporal/series") {
        Ok(jv) => jv.clone(),
        Err(e) => return e,
    };
    let step = match require_variant!(&args[1], Span, "temporal/series", "span") {
        Ok(s) => *s,
        Err(e) => return e,
    };
    let count = match require_int(&args[2], "temporal/series") {
        Ok(n) => n as usize,
        Err(e) => return e,
    };

    let mut results = Vec::with_capacity(count);
    let mut current = start;

    for i in 0..count {
        results.push(jiff_val(current.clone()));
        if i + 1 < count {
            let next = match &current {
                JiffValue::Date(d) => d.checked_add(step).map(JiffValue::Date),
                JiffValue::Time(t) => t.checked_add(step).map(JiffValue::Time),
                JiffValue::DateTime(dt) => dt.checked_add(step).map(JiffValue::DateTime),
                JiffValue::Timestamp(ts) => ts.checked_add(step).map(JiffValue::Timestamp),
                JiffValue::Zoned(z) => z
                    .as_ref()
                    .checked_add(step)
                    .map(|z| JiffValue::Zoned(Box::new(z))),
                _ => {
                    return (
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            format!(
                                "temporal/series: cannot iterate over {}",
                                current.type_name()
                            ),
                        ),
                    )
                }
            };
            match next {
                Ok(n) => current = n,
                Err(e) => return jiff_err("temporal/series", e),
            }
        }
    }

    (SIG_OK, elle::list(results))
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

pub static PRIMITIVES: &[PrimitiveDef] = &[
    // Calendar helpers
    PrimitiveDef {
        name: "date/start-of-month",
        func: prim_date_start_of_month,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "First day of the month. Works on date, datetime, zoned.",
        params: &["val"],
        category: "jiff",
        example: "(date/start-of-month (date 2024 6 19))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "date/end-of-month",
        func: prim_date_end_of_month,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Last day of the month. Works on date, datetime, zoned.",
        params: &["val"],
        category: "jiff",
        example: "(date/end-of-month (date 2024 6 19))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "date/start-of-year",
        func: prim_date_start_of_year,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "January 1 of the same year. Works on date, datetime, zoned.",
        params: &["val"],
        category: "jiff",
        example: "(date/start-of-year (date 2024 6 19))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "date/end-of-year",
        func: prim_date_end_of_year,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "December 31 of the same year. Works on date, datetime, zoned.",
        params: &["val"],
        category: "jiff",
        example: "(date/end-of-year (date 2024 6 19))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "date/next-weekday",
        func: prim_date_next_weekday,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Next occurrence of a weekday (keyword: :monday .. :sunday). Works on date, datetime, zoned.",
        params: &["val", "weekday"],
        category: "jiff",
        example: "(date/next-weekday (date 2024 6 19) :monday)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "date/prev-weekday",
        func: prim_date_prev_weekday,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Previous occurrence of a weekday (keyword: :monday .. :sunday). Works on date, datetime, zoned.",
        params: &["val", "weekday"],
        category: "jiff",
        example: "(date/prev-weekday (date 2024 6 19) :monday)",
        aliases: &[],
    },
    // Timezone
    PrimitiveDef {
        name: "tz-list",
        func: prim_tz_list,
        signal: Signal::silent(),
        arity: Arity::Exact(0),
        doc: "List all available IANA timezone names.",
        params: &[],
        category: "jiff",
        example: "(tz-list)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "tz-valid?",
        func: prim_tz_valid,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "True if the string is a valid IANA timezone name.",
        params: &["name"],
        category: "jiff",
        example: r#"(tz-valid? "America/New_York")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "tz-system",
        func: prim_tz_system,
        signal: Signal::errors(),
        arity: Arity::Exact(0),
        doc: "Return the system's IANA timezone name.",
        params: &[],
        category: "jiff",
        example: "(tz-system)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "tz-fixed",
        func: prim_tz_fixed,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Create a fixed-offset timezone string from seconds offset.",
        params: &["offset-secs"],
        category: "jiff",
        example: "(tz-fixed -18000)",
        aliases: &[],
    },
    // Series
    PrimitiveDef {
        name: "temporal/series",
        func: prim_temporal_series,
        signal: Signal::errors(),
        arity: Arity::Exact(3),
        doc: "Generate a list of temporal values: start, then start+step, start+2*step, etc.",
        params: &["start", "step", "count"],
        category: "jiff",
        example: "(temporal/series (date 2024 1 1) (span {:months 1}) 12)",
        aliases: &[],
    },
];
