// audited: 2026-11-11
//! Moves each constant out of a loop body into a `let` around the loop, so it is built once per entry.
//!
//! docs/impl/hoist.md

use super::arena::BindingArena;
use super::binding::Binding;
use super::expr::{Hir, HirKind};
use crate::signals::SIG_ERROR;
use crate::value::SymbolId;

/// The constructors of the immutable collection literals, `[…]`, `{…}`, `|…|`
/// and `b[…]`, which the analyzer lowers to a call with the items as
/// operands.
const COLLECTIONS: [&str; 4] = ["array", "struct", "set", "bytes"];

/// Move each constant in the body of an outermost loop into a `let` around
/// that loop (docs/impl/hoist.md).
pub fn hoist_loop_constants(hir: &mut Hir, arena: &mut BindingArena) {
    let collections = COLLECTIONS.map(SymbolId::of);
    Hoist { arena, collections }.outside(hir);
}

struct Hoist<'a> {
    arena: &'a mut BindingArena,
    collections: [SymbolId; 4],
}

impl Hoist<'_> {
    /// Walk code no loop of its function encloses, and hoist out of each loop
    /// it meets.
    fn outside(&mut self, hir: &mut Hir) {
        let HirKind::Loop { bindings, body } = &mut hir.kind else {
            hir.for_each_child_mut(|child| self.outside(child));
            return;
        };
        // The initial values run once on each entry, outside the passes.
        for (_, init) in bindings.iter_mut() {
            self.outside(init);
        }
        let mut moved = Vec::new();
        self.inside(body, &mut moved);
        if moved.is_empty() {
            return;
        }
        let (span, signal) = (hir.span, hir.signal);
        let lp = std::mem::replace(hir, Hir::silent(HirKind::Nil, span));
        *hir = Hir::new(
            HirKind::Let {
                bindings: moved,
                body: Box::new(lp),
            },
            span,
            signal,
        );
    }

    /// Walk a loop body, nested loops included, and replace each constant with
    /// a read of a fresh binding, which `moved` binds to it.
    fn inside(&mut self, hir: &mut Hir, moved: &mut Vec<(Binding, Hir)>) {
        if self.is_constant(hir) {
            let b = self.arena.gensym();
            self.arena.get_mut(b).is_immutable = true;
            let read = Hir::silent(HirKind::Var(b), hir.span);
            moved.push((b, std::mem::replace(hir, read)));
            return;
        }
        match &mut hir.kind {
            // A function of its own: its loops hoist within it.
            HirKind::Lambda(lambda) => self.outside(&mut lambda.body),
            // A raise builds its value on the one pass that raises.
            HirKind::Emit { signal, value } if signal.intersects(SIG_ERROR) => self.outside(value),
            _ => hir.for_each_child_mut(|child| self.inside(child, moved)),
        }
    }

    /// Whether `hir` builds an immutable value that is the same on every run.
    fn is_constant(&self, hir: &Hir) -> bool {
        match &hir.kind {
            HirKind::String(_) => true,
            HirKind::QuoteConst(template) => !template.holds_mutable(),
            // A collection literal checks its items at run time only when the
            // analyzer could not, and then it carries `:error`.
            HirKind::Call { func, args, .. } => {
                hir.signal.is_silent()
                    && self.is_collection(func)
                    && args
                        .iter()
                        .all(|a| !a.spliced && (is_immediate(&a.expr) || self.is_constant(&a.expr)))
            }
            _ => false,
        }
    }

    /// Whether `func` names the primitive that builds an immutable collection
    /// literal. A binding of the same name the program defines is no primitive.
    fn is_collection(&self, func: &Hir) -> bool {
        let HirKind::Var(b) = &func.kind else {
            return false;
        };
        let inner = self.arena.get(*b);
        inner.is_primitive && self.collections.contains(&inner.name)
    }
}

/// Whether `hir` is a literal that allocates nothing.
fn is_immediate(hir: &Hir) -> bool {
    matches!(
        hir.kind,
        HirKind::Nil
            | HirKind::EmptyList
            | HirKind::Bool(_)
            | HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::Keyword(_)
            | HirKind::Quote(_)
    )
}

#[cfg(test)]
mod tests;
