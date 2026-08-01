use super::*;

/// The stdlib/primitive ops the fused loop is built from, resolved once to this
/// unit's bindings by name (every one is an `is_primitive` global bound by
/// `bind_primitives`). If any is absent — an impossible stdlib, but cheap to
/// guard — fusion declines and every `map` call is left intact.
pub(super) struct Ops {
    /// `(@array)` — the fresh mutable accumulator.
    pub(super) at_array: Binding,
    /// `(length coll)`.
    pub(super) length: Binding,
    /// `(get coll i)`.
    pub(super) get: Binding,
    /// `(push acc elem)` — monomorphizes to `%push-array-mut` on the `@array` acc.
    pub(super) push: Binding,
    /// `(freeze acc)` — the immutable result.
    pub(super) freeze: Binding,
    /// `(< i len)`.
    pub(super) lt: Binding,
    /// `(+ i 1)`.
    pub(super) add: Binding,
}

impl Ops {
    pub(super) fn resolve(
        arena: &BindingArena,
        symbol_names: &HashMap<u32, String>,
    ) -> Option<Ops> {
        // Name → the first `is_primitive` binding for it (each global is bound
        // once by `bind_primitives`, so first-wins is exact).
        let mut prim: HashMap<&str, Binding> = HashMap::new();
        for i in 0..arena.len() as u32 {
            let b = Binding(i);
            let bi = arena.get(b);
            if bi.is_primitive {
                if let Some(name) = symbol_names.get(&bi.name.0) {
                    prim.entry(name.as_str()).or_insert(b);
                }
            }
        }
        let find = |n: &str| prim.get(n).copied();
        Some(Ops {
            at_array: find("@array")?,
            length: find("length")?,
            get: find("get")?,
            push: find("push")?,
            freeze: find("freeze")?,
            lt: find("<")?,
            add: find("+")?,
        })
    }
}
