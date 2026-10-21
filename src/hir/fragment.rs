// audited: 2026-10-21
//! An HIR body closed over the bindings it names, so it can be re-hosted in any arena.
//!
//! docs/impl/hir.md
//! docs/impl/graft.md
//!
//! A `Binding` is an index into a `BindingArena`, and the arena is not part of
//! the term, so an HIR body alone means nothing outside the unit that analyzed
//! it. An [`HirFragment`] carries its own binding table instead: every
//! `Binding` in its body indexes that table, and each entry says what the
//! binding is. [`HirFragment::close`] builds a fragment against a defining
//! arena; [`HirFragment::graft`] re-hosts one in any arena.
//!
//! [`LambdaFragment`] closes a whole function this way, one fragment per arity
//! its body dispatches on.

use crate::hir::arena::{BindingArena, BindingInner};
use crate::hir::binding::Binding;
use crate::hir::expr::{CallArg, Hir, HirKind, IntrinsicOp};
use crate::signals::Signal;
use crate::value::SymbolId;
use rustc_hash::FxHashMap;

mod lambda;
mod pattern;

pub use lambda::{ArmBody, LambdaFragment, Selected};

/// The shape of the bytes a code payload carries for a [`LambdaFragment`].
/// Move it when the fragment types change shape: the image fingerprint records
/// it, so an image written under another encoding falls back to a source boot
/// rather than reading those bytes as another body (docs/impl/image/format.md).
pub const FRAGMENT_ENCODING: u32 = 1;

/// What a fragment-local `Binding` stands for.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum FragmentBinding {
    /// A binding the body introduces — a parameter, a `let` binding, or a
    /// `match` pattern's variable. The whole `BindingInner` travels, so a graft
    /// reproduces every binding fact instead of the subset a carrier
    /// remembered to copy.
    Local(BindingInner),
    /// A free variable of the body, resolved by name in the host unit.
    Global(SymbolId),
}

/// A name a fragment holds where no value holds it: a symbol by its id, or a
/// keyword by the spelling the body wrote (docs/impl/region/template.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FragmentName<'a> {
    Symbol(SymbolId),
    Keyword(&'a str),
}

/// An HIR body whose every `Binding` indexes its own table.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HirFragment {
    body: Hir,
    bindings: Vec<FragmentBinding>,
}

/// The bindings of the defining arena that a fragment's globals stand for, by
/// fragment index. Meaningful only in the unit that closed the fragment.
pub type ClosedGlobals = Vec<(u32, Binding)>;

impl HirFragment {
    /// Close `body` over its bindings, with `params` as the bindings it takes
    /// from outside its own text. Returns the fragment plus the fragment index
    /// of each parameter, in order.
    ///
    /// `allow_intrinsic` admits a call-position `%`-intrinsic. `LambdaFragment`
    /// always admits one. Loop fusion then asks [`holds_intrinsic`](Self::holds_intrinsic)
    /// and splices such a body only under the function's `(numeric!)`
    /// declaration, whose floor on the parameters discharges the op's operand
    /// contract (docs/intrinsics.md).
    ///
    /// `None` when the body uses a form the rebuild does not model, holds an
    /// intrinsic `allow_intrinsic` refuses, or names a free variable that is not
    /// a global. The body is then not portable, and the caller leaves its
    /// function alone: an unhandled form is left un-inlined, never miscompiled.
    pub fn close(
        body: &Hir,
        params: &[Binding],
        arena: &BindingArena,
        allow_intrinsic: bool,
    ) -> Option<(Vec<u32>, Self)> {
        Self::close_with_globals(body, params, arena, allow_intrinsic)
            .map(|(params, fragment, _)| (params, fragment))
    }

    /// [`close`](Self::close), also answering which binding of `arena` each
    /// global entry was closed from, so the defining unit can graft its own
    /// fragment back onto those bindings rather than onto a name.
    pub fn close_with_globals(
        body: &Hir,
        params: &[Binding],
        arena: &BindingArena,
        allow_intrinsic: bool,
    ) -> Option<(Vec<u32>, Self, ClosedGlobals)> {
        let mut closing = Closing {
            arena,
            allow_intrinsic,
            index: FxHashMap::default(),
            bindings: Vec::new(),
            globals: Vec::new(),
        };
        let param_index = params.iter().map(|p| closing.introduce(*p).0).collect();
        let body = rebind(body, &mut closing)?;
        Some((
            param_index,
            HirFragment {
                body,
                bindings: closing.bindings,
            },
            closing.globals,
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

    /// Does the body hold a call-position `%`-intrinsic? Loop fusion admits
    /// such a body only under the function's `(numeric!)` declaration.
    pub fn holds_intrinsic(&self) -> bool {
        self.holds_intrinsic_where(|_| true)
    }

    /// Does the body hold a call-position `%`-intrinsic whose op satisfies
    /// `accept`? The graft asks for an op with a proof obligation before it
    /// grafts into a lambda nothing live reaches.
    pub fn holds_intrinsic_where(&self, accept: impl Fn(IntrinsicOp) -> bool) -> bool {
        fn walk(h: &Hir, accept: &impl Fn(IntrinsicOp) -> bool) -> bool {
            let mut found = matches!(&h.kind, HirKind::Intrinsic { op, .. } if accept(*op));
            h.for_each_child(|c| found |= walk(c, accept));
            found
        }
        walk(&self.body, &accept)
    }

    /// The globals the body calls with no spliced operand, as fragment index
    /// and name. A spliced call never grafts, so it is not among them.
    pub fn called_globals(&self) -> Vec<(u32, SymbolId)> {
        fn walk(h: &Hir, bindings: &[FragmentBinding], out: &mut Vec<(u32, SymbolId)>) {
            if let HirKind::Call { func, args, .. } = &h.kind {
                if let HirKind::Var(b) = func.kind {
                    if let FragmentBinding::Global(name) = &bindings[b.0 as usize] {
                        if !args.iter().any(|a| a.spliced) {
                            out.push((b.0, *name));
                        }
                    }
                }
            }
            h.for_each_child(|c| walk(c, bindings, out));
        }
        let mut out = Vec::new();
        walk(&self.body, &self.bindings, &mut out);
        out
    }

    /// The names the fragment holds, for a carrier to spell. Not walked yet:
    /// it answers none.
    pub fn for_each_name(&self, _f: &mut impl FnMut(FragmentName<'_>)) {}

    /// Does every free global resolve in the host unit? Asked before a graft so
    /// a caller can decline while it still has the un-spliced call in hand.
    pub fn globals_resolve(&self, globals_by_name: &FxHashMap<SymbolId, Binding>) -> bool {
        self.bindings.iter().all(|entry| match entry {
            FragmentBinding::Local(_) => true,
            FragmentBinding::Global(name) => globals_by_name.contains_key(name),
        })
    }

    /// Re-host this fragment in `arena`, resolving each global by name.
    pub fn graft(
        &self,
        arena: &mut BindingArena,
        globals_by_name: &FxHashMap<SymbolId, Binding>,
    ) -> Option<(Vec<Binding>, Hir)> {
        self.graft_with(arena, None, |_, name| globals_by_name.get(&name).copied())
    }

    /// Re-host this fragment in `arena`. Mint a binding for each local carrying
    /// its recorded metadata, resolve each global through `resolve` (given its
    /// fragment index and name), and rebuild the body with fresh `HirId`s. A
    /// reused id collides in the region walk's per-id side tables, and a
    /// fragment read back from a file carries the ids of the process that wrote
    /// it.
    ///
    /// `span`, when given, replaces every node's span. A fragment restored from
    /// the stdlib cache carries spans whose file ids another process interned,
    /// so a graft of another unit's body passes the call's span. A graft of this
    /// unit's body passes none, and keeps the spans an error it raises names.
    ///
    /// Returns the host binding for every fragment index, so a caller holding
    /// parameter indices reads its parameters straight out of it, and the
    /// rebuilt body. `None` when a global does not resolve; nothing is minted
    /// in that case.
    pub fn graft_with(
        &self,
        arena: &mut BindingArena,
        span: Option<crate::syntax::Span>,
        mut resolve: impl FnMut(u32, SymbolId) -> Option<Binding>,
    ) -> Option<(Vec<Binding>, Hir)> {
        let mut globals = FxHashMap::default();
        for (i, entry) in self.bindings.iter().enumerate() {
            if let FragmentBinding::Global(name) = entry {
                globals.insert(i, resolve(i as u32, *name)?);
            }
        }
        let host: Vec<Binding> = self
            .bindings
            .iter()
            .enumerate()
            .map(|(i, entry)| match entry {
                FragmentBinding::Local(inner) => arena.alloc_from(inner.clone()),
                FragmentBinding::Global(_) => globals[&i],
            })
            .collect();
        let body = rebind(&self.body, &mut Grafting { host: &host, span })
            .expect("close and graft share one rebuild, so a closed body rebuilds");
        Some((host, body))
    }
}

/// The binding questions a rebuild asks. `close` and `graft` answer them from
/// opposite ends: one builds the fragment's table, the other consumes it.
trait Rebind {
    /// Map a binding the body introduces.
    fn introduce(&mut self, b: Binding) -> Binding;
    /// Map a pattern variable: introduced where the pattern first names it,
    /// and the same binding again in each other alternative of an `or`.
    fn introduce_once(&mut self, b: Binding) -> Binding;
    /// Map a reference. `None` declines the whole rebuild.
    fn reference(&mut self, b: Binding) -> Option<Binding>;
    /// May a call-position `%`-intrinsic appear? Only `close` has to ask: a
    /// grafted body is one `close` already vetted.
    fn allows_intrinsic(&self) -> bool;
    /// The span a rebuilt node carries, given the one it was rebuilt from.
    fn span(&self, original: crate::syntax::Span) -> crate::syntax::Span {
        original
    }
}

/// Builds a fragment's table while rebuilding the body against it.
struct Closing<'a> {
    arena: &'a BindingArena,
    allow_intrinsic: bool,
    index: FxHashMap<Binding, u32>,
    bindings: Vec<FragmentBinding>,
    globals: ClosedGlobals,
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

    fn introduce_once(&mut self, b: Binding) -> Binding {
        match self.index.get(&b) {
            Some(&index) => Binding(index),
            None => self.introduce(b),
        }
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
        self.globals.push((fresh.0, b));
        Some(fresh)
    }

    fn allows_intrinsic(&self) -> bool {
        self.allow_intrinsic
    }
}

/// Consumes a fragment's table, already resolved to host bindings.
struct Grafting<'a> {
    host: &'a [Binding],
    span: Option<crate::syntax::Span>,
}

impl Rebind for Grafting<'_> {
    fn span(&self, original: crate::syntax::Span) -> crate::syntax::Span {
        self.span.unwrap_or(original)
    }

    fn introduce(&mut self, b: Binding) -> Binding {
        self.host[b.0 as usize]
    }

    fn introduce_once(&mut self, b: Binding) -> Binding {
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
/// its own. A `letrec` needs a value to map its own binding, so no arm below
/// takes one. A `match` arm introduces its pattern's
/// variables before its guard and body are rebuilt, and no pattern names a
/// binding of its own.
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
        HirKind::Emit { signal, value } => HirKind::Emit {
            signal: *signal,
            value: Box::new(rebind(value, r)?),
        },
        HirKind::Match { value, arms } => {
            let value = Box::new(rebind(value, r)?);
            let mut rebound = Vec::with_capacity(arms.len());
            for (pat, guard, body) in arms {
                let pat = pattern::rebind_pattern(pat, r)?;
                let guard = match guard {
                    Some(g) => Some(rebind(g, r)?),
                    None => None,
                };
                rebound.push((pat, guard, rebind(body, r)?));
            }
            HirKind::Match {
                value,
                arms: rebound,
            }
        }
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
    Some(Hir::new(kind, r.span(h.span), h.signal))
}

/// Rebuild an operand list (a `begin`/`and`/`or` body, an intrinsic's
/// arguments) under the same discipline. `None` if any element declines.
fn rebind_all(v: &[Hir], r: &mut impl Rebind) -> Option<Vec<Hir>> {
    v.iter().map(|c| rebind(c, r)).collect()
}

#[cfg(test)]
mod tests;
