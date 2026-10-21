// audited: 2026-10-21
//! The stdlib and primitive bindings a fused loop's scaffold calls, resolved once per unit by name.
//!
//! docs/impl/dissolution.md

use super::*;

/// The ops the fused loop is built from, each an `is_primitive` binding that
/// `bind_primitives` made in this unit. While the stdlib itself compiles, `push`
/// and `<` are file-scope siblings rather than primitives, so `resolve` finds
/// no binding for them and fusion leaves every chain a plain call.
pub(super) struct Ops {
    /// `(@array)` — the fresh mutable accumulator.
    pub(super) at_array: Binding,
    /// `(length coll)`.
    pub(super) length: Binding,
    /// `(get coll i)`.
    pub(super) get: Binding,
    /// `(push acc elem)`. The graft replaces the call with `push`'s body, and
    /// the fold keeps the one arm the accumulator's proven `@array` type
    /// selects, `%push-array-mut`.
    pub(super) push: Binding,
    /// `(freeze acc)` — the immutable result.
    pub(super) freeze: Binding,
    /// `(< i len)`. Every counter the scaffold advances takes the raw `%add`
    /// intrinsic instead (`Build::advance`), so no arithmetic wrapper is resolved
    /// here.
    pub(super) lt: Binding,
}

impl Ops {
    pub(super) fn resolve(arena: &BindingArena) -> Option<Ops> {
        // The first `is_primitive` binding for each name's id. `bind_primitives`
        // binds each global once, so the first is the only one. `push` and `<`
        // are stdlib exports, which no primitive table holds, so the arena is
        // where to find them.
        let mut prim: HashMap<SymbolId, Binding> = HashMap::new();
        for i in 0..arena.len() as u32 {
            let b = Binding(i);
            let bi = arena.get(b);
            if bi.is_primitive {
                prim.entry(bi.name).or_insert(b);
            }
        }
        let find = |n: &str| prim.get(&SymbolId::of(n)).copied();
        Some(Ops {
            at_array: find("@array")?,
            length: find("length")?,
            get: find("get")?,
            push: find("push")?,
            freeze: find("freeze")?,
            lt: find("<")?,
        })
    }
}
