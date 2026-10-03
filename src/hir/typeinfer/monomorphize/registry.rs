// audited: 2026-09-28
//! The dispatch-wrapper registry: each unit's wrappers by name, so a later unit's call to one monomorphizes too.
//!
//! docs/impl/dissolution.md
//! docs/impl/stdlib-cache.md

use super::*;

/// One arm of a wrapper summarized for the cross-unit registry: the container type
/// it selects on and the monomorphic op it routes to, named by `SymbolId` rather
/// than a `Binding`. A `Binding` is a per-arena index, meaningless in a later unit;
/// the op's name and the container `TyId` (a well-known `TypeInterner` constant) are
/// both stable, so the arm re-resolves against the consuming unit's own primitive
/// bindings. The operand map (`Arm::arg_src`) is not carried — it was already proven
/// to be the identity `0..arity` when the wrapper was collected, so the rewrite reuses
/// the call's args in order.
pub(crate) struct RegArm {
    pub(crate) ty: TyId,
    pub(crate) native_name: SymbolId,
    /// This arm does not monomorphize cross-unit — it is a mutable in-place `del`
    /// and stays on the wrapper's container compensation (see
    /// `is_mutable_container`). Decided at record time because an arm's `ty` is
    /// its fixed container type.
    pub(super) skip: bool,
}

/// A wrapper summarized by name for cross-unit reuse (see `RegArm`).
pub(crate) struct RegWrapper {
    pub(crate) arity: usize,
    pub(crate) arms: Vec<RegArm>,
}

/// Per-instance persistent map of dispatch wrappers, keyed by wrapper NAME. Each
/// unit's `monomorphize_dispatch_wrappers` records its locally-defined wrappers
/// here (the stdlib's `push`/`put` land in it when `stdlib.lisp` compiles), and
/// every later unit consults it, so a user→stdlib wrapper call monomorphizes
/// exactly as an intra-unit one does, and no owned-param container reference
/// strands behind a surviving wrapper.
///
/// This is compile-time-only state: the rewrite it drives leaves the direct op in
/// the HIR, so nothing here reaches the runtime. It rides on `CompileCtx` (the
/// per-instance compile context) precisely because it must outlive the single
/// compile that defined the wrapper — never on any VM/region structure.
#[derive(Default)]
pub struct DispatchWrapperRegistry {
    pub(crate) by_name: HashMap<SymbolId, RegWrapper>,
}

impl DispatchWrapperRegistry {
    /// Record a locally-collected wrapper under its name. First definition wins,
    /// so the stdlib's canonical wrapper is never clobbered by a later same-named
    /// user binding, and re-recording across compiles is a cheap no-op.
    pub(super) fn record(&mut self, name: SymbolId, w: &Wrapper, arena: &BindingArena) {
        self.by_name.entry(name).or_insert_with(|| RegWrapper {
            arity: w.arity,
            arms: w
                .arms
                .iter()
                .map(|a| {
                    let native_name = arena.get(a.native).name;
                    let is_del = crate::primitives::registration::static_name(native_name)
                        .is_some_and(|n| n.starts_with("%del"));
                    RegArm {
                        ty: a.ty,
                        native_name,
                        skip: is_mutable_container(a.ty) && is_del,
                    }
                })
                .collect(),
        });
    }
    /// Snapshot this registry for the stdlib disk cache. SymbolIds are
    /// per-process; names travel instead, re-interned on load. `TyId` is a
    /// well-known `TypeInterner` constant (stable across processes).
    pub(crate) fn to_stored(&self, symbols: &crate::symbol::SymbolTable) -> StoredDispatchRegistry {
        StoredDispatchRegistry {
            by_name: self
                .by_name
                .iter()
                .map(|(name, rw)| {
                    (
                        symbols.name(*name).unwrap_or("").to_string(),
                        StoredRegWrapper {
                            arity: rw.arity,
                            arms: rw
                                .arms
                                .iter()
                                .map(|a| StoredRegArm {
                                    ty: a.ty.0,
                                    native_name: symbols
                                        .name(a.native_name)
                                        .unwrap_or("")
                                        .to_string(),
                                    skip: a.skip,
                                })
                                .collect(),
                        },
                    )
                })
                .collect(),
        }
    }
    /// Restore a registry snapshot into this one (used by the stdlib disk
    /// cache load path; re-interns names in the loading process's table).
    pub(crate) fn restore(
        &mut self,
        stored: StoredDispatchRegistry,
        symbols: &mut crate::symbol::SymbolTable,
    ) {
        self.by_name.clear();
        for (name, rw) in stored.by_name {
            self.by_name.insert(
                symbols.intern(&name),
                RegWrapper {
                    arity: rw.arity,
                    arms: rw
                        .arms
                        .into_iter()
                        .map(|a| RegArm {
                            ty: TyId(a.ty),
                            native_name: symbols.intern(&a.native_name),
                            skip: a.skip,
                        })
                        .collect(),
                },
            );
        }
    }
}

/// Serializable snapshot of [`DispatchWrapperRegistry`] for the stdlib disk
/// cache. Names (not per-process `SymbolId`s) travel; re-interned on load.
#[derive(serde::Serialize, serde::Deserialize, Default)]
pub(crate) struct StoredDispatchRegistry {
    pub(crate) by_name: Vec<(String, StoredRegWrapper)>,
}
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct StoredRegWrapper {
    pub(crate) arity: usize,
    pub(crate) arms: Vec<StoredRegArm>,
}
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct StoredRegArm {
    pub(crate) ty: u32,
    pub(crate) native_name: String,
    pub(crate) skip: bool,
}
