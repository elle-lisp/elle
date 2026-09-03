//! An HIR body closed over the bindings it names.
//!
//! A `Binding` is an index into a `BindingArena`, and the arena is not part of
//! the term, so an HIR body alone means nothing outside the unit that analyzed
//! it. An [`HirFragment`] carries its own binding table instead: every
//! `Binding` in its body indexes that table, and each entry says what the
//! binding is. [`HirFragment::close`] builds a fragment against a defining
//! arena; [`HirFragment::graft`] re-hosts one in any arena.
//!
//! See docs/impl/hir.md § "A fragment is closed over its bindings" for the
//! design argument and the reason each admitted form is admitted.

use crate::hir::arena::{BindingArena, BindingInner};
use crate::hir::binding::Binding;
use crate::hir::expr::{CallArg, Hir, HirKind};
use crate::signals::Signal;
use crate::value::SymbolId;
use rustc_hash::FxHashMap;

/// What a fragment-local `Binding` stands for.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum FragmentBinding {
    /// A binding the body introduces — a parameter, or a `let` binding. The
    /// whole `BindingInner` travels, so a graft reproduces every binding fact
    /// instead of the subset a carrier remembered to copy.
    Local(BindingInner),
    /// A free variable of the body, resolved by name in the host unit.
    Global(SymbolId),
}

/// An HIR body whose every `Binding` indexes its own table.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HirFragment {
    body: Hir,
    bindings: Vec<FragmentBinding>,
}

impl HirFragment {
    /// Close `body` over its bindings, with `params` as the bindings it takes
    /// from outside its own text. Returns the fragment plus the fragment index
    /// of each parameter, in order.
    ///
    /// `numeric` is the defining function's `(numeric!)` declaration. It admits
    /// a call-position `%`-intrinsic, whose operand contract that declaration
    /// discharges through the parameters it floors (docs/intrinsics.md).
    ///
    /// `None` when the body uses a form the rebuild does not model, or names a
    /// free variable that is not a global. Either way the body is not portable
    /// and the caller leaves its function alone — an unhandled form is left
    /// un-inlined, never miscompiled.
    pub fn close(
        body: &Hir,
        params: &[Binding],
        arena: &BindingArena,
        numeric: bool,
    ) -> Option<(Vec<u32>, Self)> {
        let mut closing = Closing {
            arena,
            numeric,
            index: FxHashMap::default(),
            bindings: Vec::new(),
        };
        let param_index = params.iter().map(|p| closing.introduce(*p).0).collect();
        let body = rebind(body, &mut closing)?;
        Some((
            param_index,
            HirFragment {
                body,
                bindings: closing.bindings,
            },
        ))
    }

    /// The body's own signal — the fact a caller reads to decide whether
    /// splicing this body somewhere reorders an effect.
    pub fn body_signal(&self) -> Signal {
        self.body.signal
    }

    /// The fragment indices of the bindings the body introduces — exactly the
    /// ones a graft mints. A caller re-kinds them when its splice places a
    /// binding somewhere other than the defining unit did.
    pub fn local_indices(&self) -> impl Iterator<Item = u32> + '_ {
        self.bindings
            .iter()
            .enumerate()
            .filter(|(_, e)| matches!(e, FragmentBinding::Local(_)))
            .map(|(i, _)| i as u32)
    }

    /// Does every free global resolve in the host unit? Asked before a graft so
    /// a caller can decline while it still has the un-spliced call in hand.
    pub fn globals_resolve(&self, globals_by_name: &FxHashMap<SymbolId, Binding>) -> bool {
        self.bindings.iter().all(|entry| match entry {
            FragmentBinding::Local(_) => true,
            FragmentBinding::Global(name) => globals_by_name.contains_key(name),
        })
    }

    /// Re-host this fragment in `arena`: mint a binding for each local carrying
    /// its recorded metadata, resolve each global by name, and rebuild the body
    /// with fresh `HirId`s — a reused id collides in the region walk's per-id
    /// side tables, and a fragment read back from a file carries the ids of the
    /// process that wrote it.
    ///
    /// Returns the host binding for every fragment index, so a caller holding
    /// parameter indices reads its parameters straight out of it, and the
    /// rebuilt body. `None` when a global does not resolve here; nothing is
    /// minted in that case.
    pub fn graft(
        &self,
        arena: &mut BindingArena,
        globals_by_name: &FxHashMap<SymbolId, Binding>,
    ) -> Option<(Vec<Binding>, Hir)> {
        if !self.globals_resolve(globals_by_name) {
            return None;
        }
        let host: Vec<Binding> = self
            .bindings
            .iter()
            .map(|entry| match entry {
                FragmentBinding::Local(inner) => arena.alloc_from(inner.clone()),
                FragmentBinding::Global(name) => globals_by_name[name],
            })
            .collect();
        let body = rebind(&self.body, &mut Grafting { host: &host })
            .expect("close and graft share one rebuild, so a closed body rebuilds");
        Some((host, body))
    }
}

/// The two binding questions a rebuild asks. `close` and `graft` answer them
/// from opposite ends: one builds the fragment's table, the other consumes it.
trait Rebind {
    /// Map a binding the body introduces.
    fn introduce(&mut self, b: Binding) -> Binding;
    /// Map a reference. `None` declines the whole rebuild.
    fn reference(&mut self, b: Binding) -> Option<Binding>;
    /// May a call-position `%`-intrinsic appear? Only the function's own
    /// `(numeric!)` declaration admits one, and only `close` has to ask: a
    /// grafted body is one `close` already vetted.
    fn allows_intrinsic(&self) -> bool;
}

/// Builds a fragment's table while rebuilding the body against it.
struct Closing<'a> {
    arena: &'a BindingArena,
    numeric: bool,
    index: FxHashMap<Binding, u32>,
    bindings: Vec<FragmentBinding>,
}

impl Closing<'_> {
    fn push(&mut self, entry: FragmentBinding) -> Binding {
        let index = self.bindings.len() as u32;
        self.bindings.push(entry);
        Binding(index)
    }
}

impl Rebind for Closing<'_> {
    fn introduce(&mut self, b: Binding) -> Binding {
        let inner = self.arena.get(b).clone();
        let fresh = self.push(FragmentBinding::Local(inner));
        self.index.insert(b, fresh.0);
        fresh
    }

    fn reference(&mut self, b: Binding) -> Option<Binding> {
        if let Some(&index) = self.index.get(&b) {
            return Some(Binding(index));
        }
        // A free variable is portable only when it is a genuine global — a
        // module name or a primitive. Anything else is a reference to an
        // enclosing runtime local, which no other unit can name.
        let inner = self.arena.get(b);
        if !inner.is_file_scope && !inner.is_primitive {
            return None;
        }
        let fresh = self.push(FragmentBinding::Global(inner.name));
        self.index.insert(b, fresh.0);
        Some(fresh)
    }

    fn allows_intrinsic(&self) -> bool {
        self.numeric
    }
}

/// Consumes a fragment's table, already resolved to host bindings.
struct Grafting<'a> {
    host: &'a [Binding],
}

impl Rebind for Grafting<'_> {
    fn introduce(&mut self, b: Binding) -> Binding {
        self.host[b.0 as usize]
    }

    fn reference(&mut self, b: Binding) -> Option<Binding> {
        Some(self.host[b.0 as usize])
    }

    fn allows_intrinsic(&self) -> bool {
        true
    }
}

/// Rebuild a body with fresh `HirId`s, mapping every binding through `r`.
///
/// `introduce` is called at a `let`'s binding site and `reference` at every
/// `Var`. A sequential `let`'s value is rebuilt *before* its own binding is
/// introduced, so a later value sees the earlier binding and no value can map
/// its own — which is exactly what `letrec` needs and does not get, hence its
/// absence from the arms below.
///
/// `None` on any form not listed here. That list is the fragment's whole
/// contract, and `close` and `graft` share this walk, so a body that closes
/// always grafts.
fn rebind(h: &Hir, r: &mut impl Rebind) -> Option<Hir> {
    let kind = match &h.kind {
        HirKind::Nil => HirKind::Nil,
        HirKind::EmptyList => HirKind::EmptyList,
        HirKind::Bool(b) => HirKind::Bool(*b),
        HirKind::Int(n) => HirKind::Int(*n),
        HirKind::Float(f) => HirKind::Float(*f),
        HirKind::String(s) => HirKind::String(s.clone()),
        HirKind::Keyword(s) => HirKind::Keyword(s.clone()),
        HirKind::Var(b) => HirKind::Var(r.reference(*b)?),
        HirKind::Let { bindings, body } => {
            let mut rebound = Vec::with_capacity(bindings.len());
            for (b, value) in bindings {
                let value = rebind(value, r)?;
                rebound.push((r.introduce(*b), value));
            }
            HirKind::Let {
                bindings: rebound,
                body: Box::new(rebind(body, r)?),
            }
        }
        HirKind::Call {
            func,
            args,
            is_tail,
        } => {
            let func = Box::new(rebind(func, r)?);
            let mut rebound = Vec::with_capacity(args.len());
            for a in args {
                rebound.push(CallArg {
                    expr: rebind(&a.expr, r)?,
                    spliced: a.spliced,
                });
            }
            HirKind::Call {
                func,
                args: rebound,
                is_tail: *is_tail,
            }
        }
        HirKind::If {
            cond,
            then_branch,
            else_branch,
        } => HirKind::If {
            cond: Box::new(rebind(cond, r)?),
            then_branch: Box::new(rebind(then_branch, r)?),
            else_branch: Box::new(rebind(else_branch, r)?),
        },
        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            let mut rebound = Vec::with_capacity(clauses.len());
            for (c, b) in clauses {
                rebound.push((rebind(c, r)?, rebind(b, r)?));
            }
            HirKind::Cond {
                clauses: rebound,
                else_branch: match else_branch {
                    Some(e) => Some(Box::new(rebind(e, r)?)),
                    None => None,
                },
            }
        }
        HirKind::Begin(v) => HirKind::Begin(rebind_all(v, r)?),
        HirKind::And(v) => HirKind::And(rebind_all(v, r)?),
        HirKind::Or(v) => HirKind::Or(rebind_all(v, r)?),
        HirKind::Intrinsic { op, args } => {
            if !r.allows_intrinsic() {
                return None;
            }
            HirKind::Intrinsic {
                op: *op,
                args: rebind_all(args, r)?,
            }
        }
        _ => return None,
    };
    Some(Hir::new(kind, h.span, h.signal))
}

/// Rebuild an operand list (a `begin`/`and`/`or` body, an intrinsic's
/// arguments) under the same discipline. `None` if any element declines.
fn rebind_all(v: &[Hir], r: &mut impl Rebind) -> Option<Vec<Hir>> {
    v.iter().map(|c| rebind(c, r)).collect()
}

#[cfg(test)]
mod tests;
