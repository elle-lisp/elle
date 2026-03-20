//! Elle jiff plugin — date/time support via the `jiff` crate.
//!
//! All seven jiff types are wrapped in a single `JiffValue` enum stored
//! as an `External` value.  Each variant gets its own `type_name` string
//! so elle sees `"date"`, `"timestamp"`, etc.

use elle::plugin::PluginContext;
use elle::primitives::def::PrimitiveDef;
use elle::value::fiber::{SignalBits, SIG_ERROR};
use elle::value::{error_val, TableKey, Value};
use std::collections::BTreeMap;

pub mod access;
pub mod arith;
pub mod calendar;
pub mod construct;
pub mod convert;
pub mod format;
pub mod parse;
pub mod predicate;

// ---------------------------------------------------------------------------
// JiffValue — the single External data type
// ---------------------------------------------------------------------------

/// All seven jiff types packed into one enum so `as_external::<JiffValue>()`
/// gives us polymorphic dispatch.
#[derive(Clone, Debug)]
pub enum JiffValue {
    Timestamp(jiff::Timestamp),
    Date(jiff::civil::Date),
    Time(jiff::civil::Time),
    DateTime(jiff::civil::DateTime),
    Zoned(Box<jiff::Zoned>),
    Span(jiff::Span),
    SignedDuration(jiff::SignedDuration),
}

impl JiffValue {
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Timestamp(_) => "timestamp",
            Self::Date(_) => "date",
            Self::Time(_) => "time",
            Self::DateTime(_) => "datetime",
            Self::Zoned(_) => "zoned",
            Self::Span(_) => "span",
            Self::SignedDuration(_) => "signed-duration",
        }
    }
}

// ---------------------------------------------------------------------------
// Construction helpers
// ---------------------------------------------------------------------------

/// Wrap a JiffValue as an External with the correct type name.
pub fn jiff_val(v: JiffValue) -> Value {
    let name = v.type_name();
    Value::external(name, v)
}

/// Extract the JiffValue from an elle Value, or return a type error.
pub fn as_jiff<'a>(v: &'a Value) -> Option<&'a JiffValue> {
    v.as_external::<JiffValue>()
}

/// Extract with error on failure.
pub fn require_jiff<'a>(v: &'a Value, fn_name: &str) -> Result<&'a JiffValue, (SignalBits, Value)> {
    as_jiff(v).ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: expected temporal value, got {}", fn_name, v.type_name()),
            ),
        )
    })
}

/// Extract a specific JiffValue variant, returning a type error on mismatch.
macro_rules! require_variant {
    ($val:expr, $variant:ident, $fn_name:expr, $expected:expr) => {
        match crate::as_jiff($val) {
            Some(crate::JiffValue::$variant(inner)) => Ok(inner),
            Some(other) => Err((
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "{}: expected {}, got {}",
                        $fn_name,
                        $expected,
                        other.type_name()
                    ),
                ),
            )),
            None => Err((
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "{}: expected {}, got {}",
                        $fn_name,
                        $expected,
                        $val.type_name()
                    ),
                ),
            )),
        }
    };
}
pub(crate) use require_variant;

/// Extract a string argument or return a type error.
pub fn require_string(v: &Value, fn_name: &str) -> Result<String, (SignalBits, Value)> {
    v.with_string(|s| s.to_string()).ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: expected string, got {}", fn_name, v.type_name()),
            ),
        )
    })
}

/// Extract an int argument or return a type error.
pub fn require_int(v: &Value, fn_name: &str) -> Result<i64, (SignalBits, Value)> {
    v.as_int().ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: expected int, got {}", fn_name, v.type_name()),
            ),
        )
    })
}

/// Extract a float argument or return a type error.
pub fn require_float(v: &Value, fn_name: &str) -> Result<f64, (SignalBits, Value)> {
    v.as_float().ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: expected float, got {}", fn_name, v.type_name()),
            ),
        )
    })
}

/// Extract a keyword name or return a type error.
pub fn require_keyword(v: &Value, fn_name: &str) -> Result<String, (SignalBits, Value)> {
    v.as_keyword_name()
        .map(|s| s.to_string())
        .ok_or_else(|| {
            (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("{}: expected keyword, got {}", fn_name, v.type_name()),
                ),
            )
        })
}

/// Wrap a jiff error as an elle error value.
pub fn jiff_err(fn_name: &str, e: impl std::fmt::Display) -> (SignalBits, Value) {
    (
        SIG_ERROR,
        error_val("jiff-error", format!("{}: {}", fn_name, e)),
    )
}

/// Look up a keyword key in a struct value.
pub fn struct_get_kw(v: &Value, key: &str) -> Option<Value> {
    v.as_struct()
        .and_then(|m| m.get(&TableKey::Keyword(key.into())).copied())
}

/// Get an optional i64 from a keyword field of a struct.
pub fn struct_get_int(v: &Value, key: &str) -> Option<i64> {
    struct_get_kw(v, key).and_then(|v| v.as_int())
}

// ---------------------------------------------------------------------------
// Plugin init
// ---------------------------------------------------------------------------

fn all_primitives() -> Vec<&'static PrimitiveDef> {
    let mut all: Vec<&'static PrimitiveDef> = Vec::new();
    for p in construct::PRIMITIVES { all.push(p); }
    for p in predicate::PRIMITIVES { all.push(p); }
    for p in access::PRIMITIVES { all.push(p); }
    for p in parse::PRIMITIVES { all.push(p); }
    for p in format::PRIMITIVES { all.push(p); }
    for p in arith::PRIMITIVES { all.push(p); }
    for p in calendar::PRIMITIVES { all.push(p); }
    for p in convert::PRIMITIVES { all.push(p); }
    all
}

#[no_mangle]
/// # Safety
///
/// Called by Elle's plugin loader via `dlsym`.  The caller must pass a valid
/// `PluginContext` reference.
pub unsafe extern "C" fn elle_plugin_init(ctx: &mut PluginContext) -> Value {
    ctx.init_keywords();
    let mut fields = BTreeMap::new();
    for def in all_primitives() {
        ctx.register(def);
        fields.insert(
            TableKey::Keyword(def.name.into()),
            Value::native_fn(def.func),
        );
    }
    Value::struct_from(fields)
}
