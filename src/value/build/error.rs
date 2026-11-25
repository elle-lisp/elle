// audited: 2026-11-25
//! The error constructors: a struct `{:error :kind :message msg}` and its extra fields, born in a named region.
//!
//! docs/impl/region/errors.md

use std::collections::BTreeMap;

use super::{string, struct_from};
use crate::hir::region::RuntimeRegion;
use crate::value::heap::TableKey;
use crate::value::FiberHeap;
use crate::value::Value;

/// Construct an error value `{:error :kind :message msg}` born in `region` on
/// `heap` (Rule 3). The kind keyword is interned (immediate, no region).
#[inline]
pub(crate) fn error(
    heap: &mut FiberHeap,
    kind: &str,
    msg: impl Into<String>,
    region: RuntimeRegion,
) -> Value {
    error_extra(heap, kind, msg, &[], region)
}

/// Construct an error value with extra context fields, born in `region`.
#[inline]
pub(crate) fn error_extra(
    heap: &mut FiberHeap,
    kind: &str,
    msg: impl Into<String>,
    extra: &[(&str, Value)],
    region: RuntimeRegion,
) -> Value {
    let msg_val = string(heap, msg.into(), region);
    let mut fields = BTreeMap::new();
    fields.insert(TableKey::keyword("error"), Value::keyword(kind));
    fields.insert(TableKey::keyword("message"), msg_val);
    for (key, val) in extra {
        fields.insert(TableKey::keyword(key), *val);
    }
    struct_from(heap, fields, region)
}

/// The runtime no-match error for `match`, born in `region` on `heap`.
#[inline]
pub(crate) fn match_fail(heap: &mut FiberHeap, val: Value, region: RuntimeRegion) -> Value {
    error_extra(
        heap,
        "match-error",
        format!("match: no pattern matched {} ({})", val, val.type_name()),
        &[("value", val)],
        region,
    )
}
