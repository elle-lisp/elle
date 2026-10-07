// audited: 2026-10-06
//! Which lambdas live code reaches: a call inside any other lambda stays a call.
//!
//! docs/impl/graft.md

use crate::hir::binding::Binding;
use crate::hir::expr::{Hir, HirId, HirKind};
use rustc_hash::{FxHashMap, FxHashSet};

/// The lambdas live code reaches. Code outside every lambda is live. A lambda
/// written in live code is live unless it initializes a binding; then it is
/// live once live code names that binding, as a callee or as a value.
pub(super) fn live_lambdas(hir: &Hir) -> FxHashSet<HirId> {
    let mut scan = Liveness::default();
    scan.code(hir);
    scan.live
}

#[derive(Default)]
struct Liveness<'h> {
    /// Bindings live code has named.
    named: FxHashSet<Binding>,
    /// Lambda initializers of bindings nothing live has named yet.
    pending: FxHashMap<Binding, Vec<&'h Hir>>,
    live: FxHashSet<HirId>,
}

impl<'h> Liveness<'h> {
    /// Walk code known to be live.
    fn code(&mut self, h: &'h Hir) {
        match &h.kind {
            HirKind::Var(b) => self.name(*b),
            HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                for (b, init) in bindings {
                    self.bind(*b, init);
                }
                self.code(body);
            }
            HirKind::Define { binding, value } => self.bind(*binding, value),
            HirKind::Lambda(lambda) => {
                self.live.insert(h.id);
                self.code(&lambda.body);
            }
            _ => h.for_each_child(|c| self.code(c)),
        }
    }

    /// A binding's initializer runs where the binder is, but a lambda's body
    /// runs only once something names the binding.
    fn bind(&mut self, b: Binding, init: &'h Hir) {
        if !matches!(init.kind, HirKind::Lambda(_)) || self.named.contains(&b) {
            self.code(init);
        } else {
            self.pending.entry(b).or_default().push(init);
        }
    }

    fn name(&mut self, b: Binding) {
        if self.named.insert(b) {
            for init in self.pending.remove(&b).unwrap_or_default() {
                self.code(init);
            }
        }
    }
}
