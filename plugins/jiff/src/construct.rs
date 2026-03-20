//! Constructors: now, timestamp, date, time, datetime, zoned, span, signed-duration.

use crate::{
    jiff_err, jiff_val, require_int, require_jiff, require_string, struct_get_int, JiffValue,
};
use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::Arity;
use elle::value::{error_val, Value};

/// (now) → zoned (current wall-clock time with system timezone)
fn prim_now(_args: &[Value]) -> (SignalBits, Value) {
    let z = jiff::Zoned::now();
    (SIG_OK, jiff_val(JiffValue::Zoned(Box::new(z))))
}

/// (timestamp) → timestamp (current UTC instant)
fn prim_timestamp(args: &[Value]) -> (SignalBits, Value) {
    if args.is_empty() {
        return (
            SIG_OK,
            jiff_val(JiffValue::Timestamp(jiff::Timestamp::now())),
        );
    }
    // (timestamp epoch-secs epoch-nanos) — from components
    let secs = match require_int(&args[0], "timestamp") {
        Ok(n) => n,
        Err(e) => return e,
    };
    let nanos = if args.len() > 1 {
        match require_int(&args[1], "timestamp") {
            Ok(n) => n as i32,
            Err(e) => return e,
        }
    } else {
        0
    };
    match jiff::Timestamp::new(secs, nanos) {
        Ok(ts) => (SIG_OK, jiff_val(JiffValue::Timestamp(ts))),
        Err(e) => jiff_err("timestamp", e),
    }
}

/// (date y m d) → date
fn prim_date(args: &[Value]) -> (SignalBits, Value) {
    let y = match require_int(&args[0], "date") {
        Ok(n) => n as i16,
        Err(e) => return e,
    };
    let m = match require_int(&args[1], "date") {
        Ok(n) => n as i8,
        Err(e) => return e,
    };
    let d = match require_int(&args[2], "date") {
        Ok(n) => n as i8,
        Err(e) => return e,
    };
    match jiff::civil::Date::new(y, m, d) {
        Ok(date) => (SIG_OK, jiff_val(JiffValue::Date(date))),
        Err(e) => jiff_err("date", e),
    }
}

/// (time h m s) or (time h m s nanos) → time
fn prim_time(args: &[Value]) -> (SignalBits, Value) {
    let h = match require_int(&args[0], "time") {
        Ok(n) => n as i8,
        Err(e) => return e,
    };
    let m = match require_int(&args[1], "time") {
        Ok(n) => n as i8,
        Err(e) => return e,
    };
    let s = match require_int(&args[2], "time") {
        Ok(n) => n as i8,
        Err(e) => return e,
    };
    let ns = if args.len() > 3 {
        match require_int(&args[3], "time") {
            Ok(n) => n as i32,
            Err(e) => return e,
        }
    } else {
        0
    };
    match jiff::civil::Time::new(h, m, s, ns) {
        Ok(t) => (SIG_OK, jiff_val(JiffValue::Time(t))),
        Err(e) => jiff_err("time", e),
    }
}

/// (datetime y m d h min s) → datetime
fn prim_datetime(args: &[Value]) -> (SignalBits, Value) {
    let y = match require_int(&args[0], "datetime") {
        Ok(n) => n as i16,
        Err(e) => return e,
    };
    let mo = match require_int(&args[1], "datetime") {
        Ok(n) => n as i8,
        Err(e) => return e,
    };
    let d = match require_int(&args[2], "datetime") {
        Ok(n) => n as i8,
        Err(e) => return e,
    };
    let h = match require_int(&args[3], "datetime") {
        Ok(n) => n as i8,
        Err(e) => return e,
    };
    let min = match require_int(&args[4], "datetime") {
        Ok(n) => n as i8,
        Err(e) => return e,
    };
    let s = match require_int(&args[5], "datetime") {
        Ok(n) => n as i8,
        Err(e) => return e,
    };
    match jiff::civil::DateTime::new(y, mo, d, h, min, s, 0) {
        Ok(dt) => (SIG_OK, jiff_val(JiffValue::DateTime(dt))),
        Err(e) => jiff_err("datetime", e),
    }
}

/// (zoned temporal tz-string) → zoned
fn prim_zoned(args: &[Value]) -> (SignalBits, Value) {
    let tz_str = match require_string(&args[1], "zoned") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let tz = match jiff::tz::TimeZone::get(&tz_str) {
        Ok(tz) => tz,
        Err(e) => return jiff_err("zoned", e),
    };
    let jv = match require_jiff(&args[0], "zoned") {
        Ok(jv) => jv,
        Err(e) => return e,
    };
    match jv {
        JiffValue::DateTime(dt) => match dt.to_zoned(tz) {
            Ok(z) => (SIG_OK, jiff_val(JiffValue::Zoned(Box::new(z)))),
            Err(e) => jiff_err("zoned", e),
        },
        JiffValue::Timestamp(ts) => {
            let z = ts.to_zoned(tz);
            (SIG_OK, jiff_val(JiffValue::Zoned(Box::new(z))))
        }
        JiffValue::Date(d) => {
            let dt = d.to_datetime(jiff::civil::Time::midnight());
            match dt.to_zoned(tz) {
                Ok(z) => (SIG_OK, jiff_val(JiffValue::Zoned(Box::new(z)))),
                Err(e) => jiff_err("zoned", e),
            }
        }
        other => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "zoned: expected datetime, timestamp, or date as first arg, got {}",
                    other.type_name()
                ),
            ),
        ),
    }
}

/// (span opts-struct) → span
///
/// opts is a struct with keyword keys: :years, :months, :weeks, :days,
/// :hours, :minutes, :seconds, :milliseconds, :microseconds, :nanoseconds
fn prim_span(args: &[Value]) -> (SignalBits, Value) {
    let opts = &args[0];
    if opts.as_struct().is_none() {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("span: expected struct, got {}", opts.type_name()),
            ),
        );
    }

    let mut s = jiff::Span::new();

    if let Some(n) = struct_get_int(opts, "years") {
        s = match s.try_years(n) {
            Ok(s) => s,
            Err(e) => return jiff_err("span", e),
        };
    }
    if let Some(n) = struct_get_int(opts, "months") {
        s = match s.try_months(n) {
            Ok(s) => s,
            Err(e) => return jiff_err("span", e),
        };
    }
    if let Some(n) = struct_get_int(opts, "weeks") {
        s = match s.try_weeks(n) {
            Ok(s) => s,
            Err(e) => return jiff_err("span", e),
        };
    }
    if let Some(n) = struct_get_int(opts, "days") {
        s = match s.try_days(n) {
            Ok(s) => s,
            Err(e) => return jiff_err("span", e),
        };
    }
    if let Some(n) = struct_get_int(opts, "hours") {
        s = match s.try_hours(n) {
            Ok(s) => s,
            Err(e) => return jiff_err("span", e),
        };
    }
    if let Some(n) = struct_get_int(opts, "minutes") {
        s = match s.try_minutes(n) {
            Ok(s) => s,
            Err(e) => return jiff_err("span", e),
        };
    }
    if let Some(n) = struct_get_int(opts, "seconds") {
        s = match s.try_seconds(n) {
            Ok(s) => s,
            Err(e) => return jiff_err("span", e),
        };
    }
    if let Some(n) = struct_get_int(opts, "milliseconds") {
        s = match s.try_milliseconds(n) {
            Ok(s) => s,
            Err(e) => return jiff_err("span", e),
        };
    }
    if let Some(n) = struct_get_int(opts, "microseconds") {
        s = match s.try_microseconds(n) {
            Ok(s) => s,
            Err(e) => return jiff_err("span", e),
        };
    }
    if let Some(n) = struct_get_int(opts, "nanoseconds") {
        s = match s.try_nanoseconds(n) {
            Ok(s) => s,
            Err(e) => return jiff_err("span", e),
        };
    }

    (SIG_OK, jiff_val(JiffValue::Span(s)))
}

/// (signed-duration secs) or (signed-duration secs nanos) → signed-duration
fn prim_signed_duration(args: &[Value]) -> (SignalBits, Value) {
    let secs = match require_int(&args[0], "signed-duration") {
        Ok(n) => n,
        Err(e) => return e,
    };
    let nanos = if args.len() > 1 {
        match require_int(&args[1], "signed-duration") {
            Ok(n) => n as i32,
            Err(e) => return e,
        }
    } else {
        0
    };
    (
        SIG_OK,
        jiff_val(JiffValue::SignedDuration(jiff::SignedDuration::new(
            secs, nanos,
        ))),
    )
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

pub static PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "now",
        func: prim_now,
        signal: Signal::silent(),
        arity: Arity::Exact(0),
        doc: "Current wall-clock time with system timezone.",
        params: &[],
        category: "jiff",
        example: "(now)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "timestamp",
        func: prim_timestamp,
        signal: Signal::errors(),
        arity: Arity::Range(0, 2),
        doc: "Current UTC instant (no args), or construct from epoch seconds and optional nanoseconds.",
        params: &["secs?", "nanos?"],
        category: "jiff",
        example: "(timestamp)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "date",
        func: prim_date,
        signal: Signal::errors(),
        arity: Arity::Exact(3),
        doc: "Construct a calendar date from year, month, day.",
        params: &["year", "month", "day"],
        category: "jiff",
        example: "(date 2024 6 19)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "time",
        func: prim_time,
        signal: Signal::errors(),
        arity: Arity::Range(3, 4),
        doc: "Construct a wall-clock time from hour, minute, second, and optional nanoseconds.",
        params: &["hour", "minute", "second", "nanos?"],
        category: "jiff",
        example: "(time 15 22 45)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "datetime",
        func: prim_datetime,
        signal: Signal::errors(),
        arity: Arity::Exact(6),
        doc: "Construct a datetime from year, month, day, hour, minute, second.",
        params: &["year", "month", "day", "hour", "minute", "second"],
        category: "jiff",
        example: "(datetime 2024 6 19 15 22 45)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "zoned",
        func: prim_zoned,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Attach a timezone to a datetime, timestamp, or date.",
        params: &["temporal", "timezone"],
        category: "jiff",
        example: r#"(zoned (datetime 2024 6 19 15 22 45) "America/New_York")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "span",
        func: prim_span,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Construct a span from a struct of units: :years, :months, :weeks, :days, :hours, :minutes, :seconds, :milliseconds, :microseconds, :nanoseconds.",
        params: &["opts"],
        category: "jiff",
        example: "(span {:hours 1 :minutes 30})",
        aliases: &[],
    },
    PrimitiveDef {
        name: "signed-duration",
        func: prim_signed_duration,
        signal: Signal::errors(),
        arity: Arity::Range(1, 2),
        doc: "Construct an exact signed duration from seconds and optional nanoseconds.",
        params: &["secs", "nanos?"],
        category: "jiff",
        example: "(signed-duration 3600)",
        aliases: &[],
    },
];
