// audited: 2026-11-17
//! Moves each constant a function or a loop reads into one `let` at the root of the unit, so the unit builds it once.
//!
//! docs/impl/hoist.md
//! docs/impl/graft/captures.md

use super::arena::BindingArena;
use super::binding::{Binding, CaptureInfo, CaptureKind};
use super::expr::{Hir, HirKind};
use crate::signals::SIG_ERROR;
use crate::value::SymbolId;
use rustc_hash::{FxHashMap, FxHashSet};

mod key;
use key::ConstKey;

/// The constructors of the immutable collection literals, `[…]`, `{…}`, `|…|`
/// and `b[…]`, which the analyzer lowers to a call with the items as
/// operands.
const COLLECTIONS: [&str; 4] = ["array", "struct", "set", "bytes"];

/// Move each constant a function or a loop encloses into one `let` around the
/// root of the unit, and make every function between the two capture it
/// (docs/impl/hoist.md).
pub fn hoist_constants(hir: &mut Hir, arena: &mut BindingArena) {
    let mut hoist = Hoist {
        arena,
        collections: COLLECTIONS.map(SymbolId::of),
        moved: Vec::new(),
        bound: FxHashMap::default(),
        frames: Vec::new(),
    };
    hoist.walk(hir, Runs::Once);
    if hoist.moved.is_empty() {
        return;
    }
    let (span, signal) = (hir.span, hir.signal);
    let root = std::mem::replace(hir, Hir::silent(HirKind::Nil, span));
    *hir = Hir::new(
        HirKind::Let {
            bindings: hoist.moved,
            body: Box::new(root),
        },
        span,
        signal,
    );
}

/// How often the code the walk is in runs, per run of the unit.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Runs {
    /// At most once: outside every function and loop, or in a raise.
    Once,
    /// Any number of times: in a function body or a loop body.
    Again,
}

struct Hoist<'a> {
    arena: &'a mut BindingArena,
    collections: [SymbolId; 4],
    /// The bindings of the root `let`, each with the constant it holds.
    moved: Vec<(Binding, Hir)>,
    /// The binding of each distinct constant moved so far.
    bound: FxHashMap<ConstKey, Binding>,
    /// The lambdas the walk is inside, the innermost last.
    frames: Vec<Frame>,
}

/// A lambda the walk is inside: what it captures, and the captures the walk
/// added to it, in order.
struct Frame {
    captured: FxHashSet<Binding>,
    added: Vec<Binding>,
}

impl Hoist<'_> {
    fn walk(&mut self, hir: &mut Hir, runs: Runs) {
        if runs == Runs::Again && self.is_constant(hir) {
            let b = self.bind(hir);
            self.capture(b);
            return;
        }
        match &mut hir.kind {
            HirKind::Lambda(lambda) => {
                self.frames.push(Frame {
                    captured: lambda.captures.iter().map(|c| c.binding).collect(),
                    added: Vec::new(),
                });
                self.walk(&mut lambda.body, Runs::Again);
                let frame = self.frames.pop().expect("the frame pushed above");
                // After the analyzer's captures, so every environment index
                // they hold stays where it is.
                lambda
                    .captures
                    .extend(frame.added.into_iter().map(|binding| CaptureInfo {
                        binding,
                        kind: CaptureKind::Local,
                    }));
            }
            HirKind::Loop { bindings, body } => {
                // The initial values run once on each entry, as the code
                // around the loop does.
                for (_, init) in bindings.iter_mut() {
                    self.walk(init, runs);
                }
                self.walk(body, Runs::Again);
            }
            // A raise builds its value on the one run that raises.
            HirKind::Emit { signal, value } if signal.intersects(SIG_ERROR) => {
                self.walk(value, Runs::Once)
            }
            _ => hir.for_each_child_mut(|child| self.walk(child, runs)),
        }
    }

    /// Replace the constant `hir` with a read of the binding that holds it,
    /// minted for the first constant of its contents.
    fn bind(&mut self, hir: &mut Hir) -> Binding {
        let span = hir.span;
        let read = |b| Hir::silent(HirKind::Var(b), span);
        let key = ConstKey::of(hir);
        if let Some(&b) = self.bound.get(&key) {
            *hir = read(b);
            return b;
        }
        let b = self.arena.gensym();
        self.arena.get_mut(b).is_immutable = true;
        self.bound.insert(key, b);
        let constant = std::mem::replace(hir, read(b));
        self.moved.push((b, constant));
        b
    }

    /// Capture `b` in each lambda the walk is inside, since the root `let`
    /// that binds it is outside them all. The capture holds the value: the
    /// binding is immutable and bound before any closure is built, so it
    /// needs no cell.
    fn capture(&mut self, b: Binding) {
        if self.frames.is_empty() {
            return;
        }
        self.arena.get_mut(b).mark_captured();
        for frame in &mut self.frames {
            if frame.captured.insert(b) {
                frame.added.push(b);
            }
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
