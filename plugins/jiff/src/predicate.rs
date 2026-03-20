//! Type predicates: date?, time?, datetime?, timestamp?, zoned?, span?,
//! signed-duration?, temporal?

use crate::{as_jiff, JiffValue};
use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::fiber::{SignalBits, SIG_OK};
use elle::value::types::Arity;
use elle::value::Value;

fn prim_date_p(args: &[Value]) -> (SignalBits, Value) {
    (
        SIG_OK,
        Value::bool(matches!(as_jiff(&args[0]), Some(JiffValue::Date(_)))),
    )
}

fn prim_time_p(args: &[Value]) -> (SignalBits, Value) {
    (
        SIG_OK,
        Value::bool(matches!(as_jiff(&args[0]), Some(JiffValue::Time(_)))),
    )
}

fn prim_datetime_p(args: &[Value]) -> (SignalBits, Value) {
    (
        SIG_OK,
        Value::bool(matches!(as_jiff(&args[0]), Some(JiffValue::DateTime(_)))),
    )
}

fn prim_timestamp_p(args: &[Value]) -> (SignalBits, Value) {
    (
        SIG_OK,
        Value::bool(matches!(as_jiff(&args[0]), Some(JiffValue::Timestamp(_)))),
    )
}

fn prim_zoned_p(args: &[Value]) -> (SignalBits, Value) {
    (
        SIG_OK,
        Value::bool(matches!(as_jiff(&args[0]), Some(JiffValue::Zoned(_)))),
    )
}

fn prim_span_p(args: &[Value]) -> (SignalBits, Value) {
    (
        SIG_OK,
        Value::bool(matches!(as_jiff(&args[0]), Some(JiffValue::Span(_)))),
    )
}

fn prim_signed_duration_p(args: &[Value]) -> (SignalBits, Value) {
    (
        SIG_OK,
        Value::bool(matches!(
            as_jiff(&args[0]),
            Some(JiffValue::SignedDuration(_))
        )),
    )
}

fn prim_temporal_p(args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::bool(as_jiff(&args[0]).is_some()))
}

pub static PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "date?",
        func: prim_date_p,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "True if value is a date.",
        params: &["value"],
        category: "jiff",
        example: "(date? (date 2024 6 19))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "time?",
        func: prim_time_p,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "True if value is a time.",
        params: &["value"],
        category: "jiff",
        example: "(time? (time 15 22 45))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "datetime?",
        func: prim_datetime_p,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "True if value is a datetime.",
        params: &["value"],
        category: "jiff",
        example: "(datetime? (datetime 2024 6 19 15 22 45))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "timestamp?",
        func: prim_timestamp_p,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "True if value is a timestamp.",
        params: &["value"],
        category: "jiff",
        example: "(timestamp? (timestamp))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "zoned?",
        func: prim_zoned_p,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "True if value is a zoned datetime.",
        params: &["value"],
        category: "jiff",
        example: r#"(zoned? (now))"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "span?",
        func: prim_span_p,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "True if value is a span.",
        params: &["value"],
        category: "jiff",
        example: "(span? (span {:hours 1}))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "signed-duration?",
        func: prim_signed_duration_p,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "True if value is a signed-duration.",
        params: &["value"],
        category: "jiff",
        example: "(signed-duration? (signed-duration 3600))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "temporal?",
        func: prim_temporal_p,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "True if value is any jiff temporal type.",
        params: &["value"],
        category: "jiff",
        example: "(temporal? (now))",
        aliases: &[],
    },
];
