//! Component accessors: date/year, date/month, date/day, time/hour, etc.
//!
//! Many accessors are polymorphic — `date/year` works on date, datetime, and
//! zoned.  The pattern: extract JiffValue, match on variants that support the
//! operation, error on others.

use crate::{as_jiff, require_variant, JiffValue};
use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::Arity;
use elle::value::{error_val, Value};

// ---------------------------------------------------------------------------
// Date component helpers (work on Date, DateTime, Zoned)
// ---------------------------------------------------------------------------

macro_rules! date_accessor {
    ($fn_name:ident, $prim_name:expr, $method:ident, $cast:ty) => {
        fn $fn_name(args: &[Value]) -> (SignalBits, Value) {
            match as_jiff(&args[0]) {
                Some(JiffValue::Date(d)) => (SIG_OK, Value::int(d.$method() as i64)),
                Some(JiffValue::DateTime(dt)) => (SIG_OK, Value::int(dt.$method() as i64)),
                Some(JiffValue::Zoned(z)) => (SIG_OK, Value::int(z.$method() as i64)),
                Some(other) => (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "{}: expected date, datetime, or zoned, got {}",
                            $prim_name,
                            other.type_name()
                        ),
                    ),
                ),
                None => (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "{}: expected date, datetime, or zoned, got {}",
                            $prim_name,
                            args[0].type_name()
                        ),
                    ),
                ),
            }
        }
    };
}

date_accessor!(prim_date_year, "date/year", year, i16);
date_accessor!(prim_date_month, "date/month", month, i8);
date_accessor!(prim_date_day, "date/day", day, i8);

// date/weekday returns a keyword
fn prim_date_weekday(args: &[Value]) -> (SignalBits, Value) {
    let wd = match as_jiff(&args[0]) {
        Some(JiffValue::Date(d)) => d.weekday(),
        Some(JiffValue::DateTime(dt)) => dt.weekday(),
        Some(JiffValue::Zoned(z)) => z.weekday(),
        Some(other) => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "date/weekday: expected date, datetime, or zoned, got {}",
                        other.type_name()
                    ),
                ),
            )
        }
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "date/weekday: expected date, datetime, or zoned, got {}",
                        args[0].type_name()
                    ),
                ),
            )
        }
    };
    let name = match wd {
        jiff::civil::Weekday::Monday => "monday",
        jiff::civil::Weekday::Tuesday => "tuesday",
        jiff::civil::Weekday::Wednesday => "wednesday",
        jiff::civil::Weekday::Thursday => "thursday",
        jiff::civil::Weekday::Friday => "friday",
        jiff::civil::Weekday::Saturday => "saturday",
        jiff::civil::Weekday::Sunday => "sunday",
    };
    (SIG_OK, Value::keyword(name))
}

fn prim_date_weekday_number(args: &[Value]) -> (SignalBits, Value) {
    let wd = match as_jiff(&args[0]) {
        Some(JiffValue::Date(d)) => d.weekday(),
        Some(JiffValue::DateTime(dt)) => dt.weekday(),
        Some(JiffValue::Zoned(z)) => z.weekday(),
        Some(other) => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "date/weekday-number: expected date, datetime, or zoned, got {}",
                        other.type_name()
                    ),
                ),
            )
        }
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "date/weekday-number: expected date, datetime, or zoned, got {}",
                        args[0].type_name()
                    ),
                ),
            )
        }
    };
    (SIG_OK, Value::int(wd.to_monday_one_offset() as i64))
}

fn prim_date_day_of_year(args: &[Value]) -> (SignalBits, Value) {
    let n = match as_jiff(&args[0]) {
        Some(JiffValue::Date(d)) => d.day_of_year(),
        Some(JiffValue::DateTime(dt)) => dt.date().day_of_year(),
        Some(JiffValue::Zoned(z)) => z.date().day_of_year(),
        Some(other) => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "date/day-of-year: expected date, datetime, or zoned, got {}",
                        other.type_name()
                    ),
                ),
            )
        }
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "date/day-of-year: expected date, datetime, or zoned, got {}",
                        args[0].type_name()
                    ),
                ),
            )
        }
    };
    (SIG_OK, Value::int(n as i64))
}

fn prim_date_days_in_month(args: &[Value]) -> (SignalBits, Value) {
    let n = match as_jiff(&args[0]) {
        Some(JiffValue::Date(d)) => d.days_in_month(),
        Some(JiffValue::DateTime(dt)) => dt.date().days_in_month(),
        Some(JiffValue::Zoned(z)) => z.date().days_in_month(),
        Some(other) => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "date/days-in-month: expected date, datetime, or zoned, got {}",
                        other.type_name()
                    ),
                ),
            )
        }
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "date/days-in-month: expected date, datetime, or zoned, got {}",
                        args[0].type_name()
                    ),
                ),
            )
        }
    };
    (SIG_OK, Value::int(n as i64))
}

fn prim_date_days_in_year(args: &[Value]) -> (SignalBits, Value) {
    let n = match as_jiff(&args[0]) {
        Some(JiffValue::Date(d)) => d.days_in_year(),
        Some(JiffValue::DateTime(dt)) => dt.date().days_in_year(),
        Some(JiffValue::Zoned(z)) => z.date().days_in_year(),
        Some(other) => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "date/days-in-year: expected date, datetime, or zoned, got {}",
                        other.type_name()
                    ),
                ),
            )
        }
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "date/days-in-year: expected date, datetime, or zoned, got {}",
                        args[0].type_name()
                    ),
                ),
            )
        }
    };
    (SIG_OK, Value::int(n as i64))
}

fn prim_date_leap_year(args: &[Value]) -> (SignalBits, Value) {
    let b = match as_jiff(&args[0]) {
        Some(JiffValue::Date(d)) => d.in_leap_year(),
        Some(JiffValue::DateTime(dt)) => dt.date().in_leap_year(),
        Some(JiffValue::Zoned(z)) => z.date().in_leap_year(),
        Some(other) => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "date/leap-year?: expected date, datetime, or zoned, got {}",
                        other.type_name()
                    ),
                ),
            )
        }
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "date/leap-year?: expected date, datetime, or zoned, got {}",
                        args[0].type_name()
                    ),
                ),
            )
        }
    };
    (SIG_OK, Value::bool(b))
}

// ---------------------------------------------------------------------------
// Time component helpers (work on Time, DateTime, Zoned)
// ---------------------------------------------------------------------------

macro_rules! time_accessor {
    ($fn_name:ident, $prim_name:expr, $method:ident) => {
        fn $fn_name(args: &[Value]) -> (SignalBits, Value) {
            match as_jiff(&args[0]) {
                Some(JiffValue::Time(t)) => (SIG_OK, Value::int(t.$method() as i64)),
                Some(JiffValue::DateTime(dt)) => (SIG_OK, Value::int(dt.$method() as i64)),
                Some(JiffValue::Zoned(z)) => (SIG_OK, Value::int(z.$method() as i64)),
                Some(other) => (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "{}: expected time, datetime, or zoned, got {}",
                            $prim_name,
                            other.type_name()
                        ),
                    ),
                ),
                None => (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "{}: expected time, datetime, or zoned, got {}",
                            $prim_name,
                            args[0].type_name()
                        ),
                    ),
                ),
            }
        }
    };
}

time_accessor!(prim_time_hour, "time/hour", hour);
time_accessor!(prim_time_minute, "time/minute", minute);
time_accessor!(prim_time_second, "time/second", second);
time_accessor!(prim_time_millisecond, "time/millisecond", millisecond);
time_accessor!(prim_time_microsecond, "time/microsecond", microsecond);
time_accessor!(prim_time_nanosecond, "time/nanosecond", nanosecond);
time_accessor!(prim_time_subsec_nanosecond, "time/subsec-nanosecond", subsec_nanosecond);

// ---------------------------------------------------------------------------
// Zoned-specific accessors
// ---------------------------------------------------------------------------

fn prim_zoned_tz_name(args: &[Value]) -> (SignalBits, Value) {
    let z = match require_variant!(&args[0], Zoned, "zoned/tz-name", "zoned") {
        Ok(z) => z,
        Err(e) => return e,
    };
    (SIG_OK, Value::string(z.time_zone().iana_name().unwrap_or("unknown")))
}

fn prim_zoned_utc_offset(args: &[Value]) -> (SignalBits, Value) {
    let z = match require_variant!(&args[0], Zoned, "zoned/utc-offset", "zoned") {
        Ok(z) => z,
        Err(e) => return e,
    };
    (SIG_OK, Value::int(z.offset().seconds() as i64))
}

// ---------------------------------------------------------------------------
// SignedDuration accessors
// ---------------------------------------------------------------------------

fn prim_sd_secs(args: &[Value]) -> (SignalBits, Value) {
    let d = match require_variant!(&args[0], SignedDuration, "signed-duration/secs", "signed-duration") {
        Ok(d) => d,
        Err(e) => return e,
    };
    (SIG_OK, Value::int(d.as_secs()))
}

fn prim_sd_nanos(args: &[Value]) -> (SignalBits, Value) {
    let d = match require_variant!(&args[0], SignedDuration, "signed-duration/nanos", "signed-duration") {
        Ok(d) => d,
        Err(e) => return e,
    };
    (SIG_OK, Value::int(d.subsec_nanos() as i64))
}

fn prim_sd_zero(args: &[Value]) -> (SignalBits, Value) {
    let d = match require_variant!(&args[0], SignedDuration, "signed-duration/zero?", "signed-duration") {
        Ok(d) => d,
        Err(e) => return e,
    };
    (SIG_OK, Value::bool(d.is_zero()))
}

// ---------------------------------------------------------------------------
// Span accessors
// ---------------------------------------------------------------------------

fn prim_span_get(args: &[Value]) -> (SignalBits, Value) {
    let s = match require_variant!(&args[0], Span, "span/get", "span") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let unit = match crate::require_keyword(&args[1], "span/get") {
        Ok(k) => k,
        Err(e) => return e,
    };
    let n = match unit.as_str() {
        "years" => s.get_years() as i64,
        "months" => s.get_months() as i64,
        "weeks" => s.get_weeks() as i64,
        "days" => s.get_days() as i64,
        "hours" => s.get_hours() as i64,
        "minutes" => s.get_minutes() as i64,
        "seconds" => s.get_seconds() as i64,
        "milliseconds" => s.get_milliseconds() as i64,
        "microseconds" => s.get_microseconds() as i64,
        "nanoseconds" => s.get_nanoseconds() as i64,
        other => {
            return (
                SIG_ERROR,
                error_val(
                    "jiff-error",
                    format!("span/get: unknown unit {:?}", other),
                ),
            )
        }
    };
    (SIG_OK, Value::int(n))
}

fn prim_span_zero(args: &[Value]) -> (SignalBits, Value) {
    let s = match require_variant!(&args[0], Span, "span/zero?", "span") {
        Ok(s) => s,
        Err(e) => return e,
    };
    (SIG_OK, Value::bool(s.is_zero()))
}

fn prim_span_to_struct(args: &[Value]) -> (SignalBits, Value) {
    use elle::value::TableKey;
    use std::collections::BTreeMap;

    let s = match require_variant!(&args[0], Span, "span->struct", "span") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let mut fields = BTreeMap::new();
    fields.insert(TableKey::Keyword("years".into()), Value::int(s.get_years() as i64));
    fields.insert(TableKey::Keyword("months".into()), Value::int(s.get_months() as i64));
    fields.insert(TableKey::Keyword("weeks".into()), Value::int(s.get_weeks() as i64));
    fields.insert(TableKey::Keyword("days".into()), Value::int(s.get_days() as i64));
    fields.insert(TableKey::Keyword("hours".into()), Value::int(s.get_hours() as i64));
    fields.insert(TableKey::Keyword("minutes".into()), Value::int(s.get_minutes() as i64));
    fields.insert(TableKey::Keyword("seconds".into()), Value::int(s.get_seconds() as i64));
    fields.insert(TableKey::Keyword("milliseconds".into()), Value::int(s.get_milliseconds() as i64));
    fields.insert(TableKey::Keyword("microseconds".into()), Value::int(s.get_microseconds() as i64));
    fields.insert(TableKey::Keyword("nanoseconds".into()), Value::int(s.get_nanoseconds() as i64));
    (SIG_OK, Value::struct_from(fields))
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

pub static PRIMITIVES: &[PrimitiveDef] = &[
    // Date components
    PrimitiveDef {
        name: "date/year",
        func: prim_date_year,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Year component. Works on date, datetime, zoned.",
        params: &["val"],
        category: "jiff",
        example: "(date/year (date 2024 6 19))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "date/month",
        func: prim_date_month,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Month component (1-12). Works on date, datetime, zoned.",
        params: &["val"],
        category: "jiff",
        example: "(date/month (date 2024 6 19))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "date/day",
        func: prim_date_day,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Day component (1-31). Works on date, datetime, zoned.",
        params: &["val"],
        category: "jiff",
        example: "(date/day (date 2024 6 19))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "date/weekday",
        func: prim_date_weekday,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Day of week as keyword (:monday .. :sunday). Works on date, datetime, zoned.",
        params: &["val"],
        category: "jiff",
        example: "(date/weekday (date 2024 6 19))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "date/weekday-number",
        func: prim_date_weekday_number,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "ISO weekday number (1=Monday .. 7=Sunday). Works on date, datetime, zoned.",
        params: &["val"],
        category: "jiff",
        example: "(date/weekday-number (date 2024 6 19))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "date/day-of-year",
        func: prim_date_day_of_year,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Day of year (1-366). Works on date, datetime, zoned.",
        params: &["val"],
        category: "jiff",
        example: "(date/day-of-year (date 2024 6 19))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "date/days-in-month",
        func: prim_date_days_in_month,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Number of days in the month (28-31). Works on date, datetime, zoned.",
        params: &["val"],
        category: "jiff",
        example: "(date/days-in-month (date 2024 2 1))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "date/days-in-year",
        func: prim_date_days_in_year,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Number of days in the year (365 or 366). Works on date, datetime, zoned.",
        params: &["val"],
        category: "jiff",
        example: "(date/days-in-year (date 2024 1 1))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "date/leap-year?",
        func: prim_date_leap_year,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "True if the year is a leap year. Works on date, datetime, zoned.",
        params: &["val"],
        category: "jiff",
        example: "(date/leap-year? (date 2024 1 1))",
        aliases: &[],
    },
    // Time components
    PrimitiveDef {
        name: "time/hour",
        func: prim_time_hour,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Hour component (0-23). Works on time, datetime, zoned.",
        params: &["val"],
        category: "jiff",
        example: "(time/hour (time 15 22 45))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "time/minute",
        func: prim_time_minute,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Minute component (0-59). Works on time, datetime, zoned.",
        params: &["val"],
        category: "jiff",
        example: "(time/minute (time 15 22 45))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "time/second",
        func: prim_time_second,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Second component (0-59). Works on time, datetime, zoned.",
        params: &["val"],
        category: "jiff",
        example: "(time/second (time 15 22 45))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "time/millisecond",
        func: prim_time_millisecond,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Millisecond component (0-999). Works on time, datetime, zoned.",
        params: &["val"],
        category: "jiff",
        example: "(time/millisecond (time 15 22 45 123456789))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "time/microsecond",
        func: prim_time_microsecond,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Microsecond component (0-999999). Works on time, datetime, zoned.",
        params: &["val"],
        category: "jiff",
        example: "(time/microsecond (time 15 22 45 123456789))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "time/nanosecond",
        func: prim_time_nanosecond,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Nanosecond component (0-999999999). Works on time, datetime, zoned.",
        params: &["val"],
        category: "jiff",
        example: "(time/nanosecond (time 15 22 45 123456789))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "time/subsec-nanosecond",
        func: prim_time_subsec_nanosecond,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Sub-second nanoseconds. Works on time, datetime, zoned.",
        params: &["val"],
        category: "jiff",
        example: "(time/subsec-nanosecond (time 15 22 45 123456789))",
        aliases: &[],
    },
    // Zoned-specific
    PrimitiveDef {
        name: "zoned/tz-name",
        func: prim_zoned_tz_name,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "IANA timezone name of a zoned datetime.",
        params: &["val"],
        category: "jiff",
        example: "(zoned/tz-name (now))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "zoned/utc-offset",
        func: prim_zoned_utc_offset,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "UTC offset in seconds of a zoned datetime.",
        params: &["val"],
        category: "jiff",
        example: "(zoned/utc-offset (now))",
        aliases: &[],
    },
    // SignedDuration accessors
    PrimitiveDef {
        name: "signed-duration/secs",
        func: prim_sd_secs,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Whole seconds of a signed-duration.",
        params: &["val"],
        category: "jiff",
        example: "(signed-duration/secs (signed-duration 3661 500000000))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "signed-duration/nanos",
        func: prim_sd_nanos,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Sub-second nanoseconds of a signed-duration.",
        params: &["val"],
        category: "jiff",
        example: "(signed-duration/nanos (signed-duration 3661 500000000))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "signed-duration/zero?",
        func: prim_sd_zero,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "True if the signed-duration is zero.",
        params: &["val"],
        category: "jiff",
        example: "(signed-duration/zero? (signed-duration 0))",
        aliases: &[],
    },
    // Span accessors
    PrimitiveDef {
        name: "span/get",
        func: prim_span_get,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Get a unit field from a span. Unit is a keyword: :years, :months, :weeks, :days, :hours, :minutes, :seconds, :milliseconds, :microseconds, :nanoseconds.",
        params: &["span", "unit"],
        category: "jiff",
        example: "(span/get (span {:hours 1 :minutes 30}) :hours)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "span/zero?",
        func: prim_span_zero,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "True if all span fields are zero.",
        params: &["span"],
        category: "jiff",
        example: "(span/zero? (span {:hours 0}))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "span->struct",
        func: prim_span_to_struct,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Convert a span to a struct with all 10 unit fields.",
        params: &["span"],
        category: "jiff",
        example: "(span->struct (span {:hours 1 :minutes 30}))",
        aliases: &[],
    },
];
