// audited: 2026-11-09
//! `fn/captures`: one struct per captured slot of a closure, with its value, its name and whether code assigns it.
//!
//! docs/modules.md
//! docs/modules-proposal/shape.md

use crate::value::fiber::{SignalBits, SIG_OK};
use crate::value::heap::{CellOrigin, HeapObject};
use crate::value::{SymbolId, TableKey, Value};

/// (fn/captures value) — one struct per captured slot of a closure, or nil.
///
/// A slot answers `{:value v :mutable? b}`, and `:name` beside them when the
/// code object recorded the binding. The value is what the slot holds now: a
/// capture cell's content, never the cell. `:mutable?` is the record's answer
/// to whether code assigns the binding.
///
/// A slot no record names answers from what it holds. A compiled forward cell
/// carries its binding's name and mark; any other cell may be assigned, so it
/// answers true; a plain value answers false.
pub(crate) fn prim_captures(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let Some(closure) = args[0].as_closure() else {
        return (SIG_OK, Value::NIL);
    };
    let facts = closure.template.payload().captures();
    let slots: Vec<(Value, Option<SymbolId>, bool)> = closure
        .env
        .as_slice()
        .iter()
        .enumerate()
        .map(|(i, &slot)| {
            let value = slot.capture_cell_get().unwrap_or(slot);
            match facts.get(i) {
                Some(fact) => (value, Some(fact.name()), fact.assigned()),
                None => match cell_origin(slot) {
                    Some(CellOrigin::Compiled { name, mutated }) => (value, Some(name), mutated),
                    Some(CellOrigin::Runtime) => (value, None, true),
                    None => (value, None, false),
                },
            }
        })
        .collect();
    let mut records = Vec::with_capacity(slots.len());
    for (value, name, mutable) in slots {
        let mut fields = std::collections::BTreeMap::new();
        fields.insert(key(ctx, "value"), value);
        fields.insert(key(ctx, "mutable?"), Value::bool(mutable));
        let spelled = name.and_then(|id| ctx.vm().symbols().and_then(|s| s.name(id)));
        if let Some(spelled) = spelled.map(str::to_string) {
            let spelled = ctx.string(spelled);
            fields.insert(key(ctx, "name"), spelled);
        }
        records.push(ctx.struct_from(fields));
    }
    (SIG_OK, ctx.array(records))
}

/// Where a capture cell came from, or `None` for a slot that holds no cell.
fn cell_origin(slot: Value) -> Option<CellOrigin> {
    if !slot.is_capture_cell() {
        return None;
    }
    match unsafe { crate::value::arena::deref(slot) } {
        HeapObject::CaptureCell { origin, .. } => Some(*origin),
        _ => None,
    }
}

/// The struct key a keyword spelled `name` makes, learned by this instance.
fn key(ctx: &mut crate::primitives::ctx::NativeCtx<'_>, name: &str) -> TableKey {
    TableKey::from_value(&ctx.keyword(name)).expect("a keyword is a struct key")
}
