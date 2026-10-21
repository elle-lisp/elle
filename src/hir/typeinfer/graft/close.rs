// audited: 2026-10-21
//! Closes every lambda of the unit into the fragment its code payload will carry, and names the functions a call may graft.
//!
//! docs/impl/graft.md
//! docs/impl/dissolution/inline.md

use super::super::fuse::classify_base;
use super::super::prune::concrete_init_keywords;
use crate::hir::arena::BindingArena;
use crate::hir::binding::Binding;
use crate::hir::expr::{Hir, HirKind};
use crate::hir::fragment::{ClosedGlobals, LambdaFragment};
use crate::hir::Lambda;
use crate::value::SymbolId;
use rustc_hash::{FxHashMap, FxHashSet};
use std::rc::Rc;

/// A function this unit defines, as a call to it grafts or fuses it.
pub(crate) struct UnitFn {
    /// The binding this unit defines the function under.
    pub(crate) binding: Binding,
    pub(crate) fragment: Rc<LambdaFragment>,
    /// Per arm of the fragment, the bindings of this unit its globals stand
    /// for, so a graft here re-hosts them on the very bindings they came from.
    pub(crate) globals: Vec<ClosedGlobals>,
    /// The function's fixed parameters, as this unit's tree names them.
    pub(crate) params: Vec<Binding>,
    pub(crate) rest: Option<Binding>,
    /// Per arm: it calls the function again, directly or through a cycle of
    /// this unit's functions.
    pub(crate) recursive_arms: Vec<bool>,
}

/// This unit's functions by the binding a call names them with.
pub(crate) type UnitFns = FxHashMap<Binding, UnitFn>;

/// Attach a fragment to every lambda in the tree whose body closes, and collect
/// the ones bound once to an immutable, unmutated binding: the functions a
/// call may graft.
///
/// This runs after dead-arm pruning and before fusion and the graft, so a
/// fragment is the body as the programmer wrote it, dead arms pruned.
pub(crate) fn close_lambdas(hir: &mut Hir, arena: &BindingArena) -> UnitFns {
    let bases = concrete_init_keywords(hir, arena);
    let mut closer = Closer {
        arena,
        bases: &bases,
        sites: FxHashMap::default(),
        unit: FxHashMap::default(),
    };
    closer.walk(hir, None);
    let mut unit = closer.unit;
    unit.retain(|b, _| closer.sites.get(b) == Some(&1));
    mark_recursive_arms(hir, &mut unit);
    unit
}

struct Closer<'a> {
    arena: &'a BindingArena,
    bases: &'a FxHashMap<Binding, &'static str>,
    /// How many binding sites introduce each binding: one bound twice has no
    /// single body a call reaches.
    sites: FxHashMap<Binding, u32>,
    unit: UnitFns,
}

impl Closer<'_> {
    /// `name` is the binding whose initializer `h` is, when it is one.
    fn walk(&mut self, h: &mut Hir, name: Option<Binding>) {
        match &mut h.kind {
            HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                for (b, init) in bindings.iter_mut() {
                    *self.sites.entry(*b).or_default() += 1;
                    self.walk(init, Some(*b));
                }
                self.walk(body, None);
            }
            HirKind::Define { binding, value } => {
                *self.sites.entry(*binding).or_default() += 1;
                self.walk(value, Some(*binding));
            }
            HirKind::Lambda(lambda) => {
                self.walk(&mut lambda.body, None);
                self.close(h, name);
            }
            _ => h.for_each_child_mut(|c| self.walk(c, None)),
        }
    }

    fn close(&mut self, lambda: &mut Hir, bound: Option<Binding>) {
        let name: Option<SymbolId> = bound.map(|b| self.arena.get(b).name);
        let HirKind::Lambda(node) = &lambda.kind else {
            return;
        };
        let returns_array = classify_base(&node.body, self.arena, self.bases).is_some();
        let Some((closed, globals)) =
            LambdaFragment::close(lambda, name, self.arena, returns_array)
        else {
            return;
        };
        let closed = Rc::new(closed);
        let Some(Lambda {
            params,
            rest_param,
            fragment,
            ..
        }) = lambda.kind.as_lambda_mut()
        else {
            unreachable!("matched a lambda above");
        };
        *fragment = Some(Rc::clone(&closed));
        let Some(b) = bound else { return };
        let inner = self.arena.get(b);
        if !inner.is_immutable || inner.is_mutated {
            return;
        }
        let rest = *rest_param;
        let params = params
            .iter()
            .copied()
            .filter(|p| Some(*p) != rest)
            .collect();
        self.unit.insert(
            b,
            UnitFn {
                binding: b,
                fragment: closed,
                globals,
                params,
                rest,
                recursive_arms: Vec::new(),
            },
        );
    }
}

/// Mark each arm of each function of `unit` that calls the function again:
/// directly, or through calls to functions of `unit` that reach back to it.
/// Only a call with no spliced operand counts, because only such a call can
/// graft, and so only it could unroll.
fn mark_recursive_arms(hir: &Hir, unit: &mut UnitFns) {
    let mut calls: FxHashMap<Binding, Vec<Binding>> = FxHashMap::default();
    collect_calls(hir, unit, &mut calls);
    let reaches = |from: Binding, to: Binding| {
        let mut seen = FxHashSet::default();
        let mut stack = vec![from];
        while let Some(f) = stack.pop() {
            if f == to {
                return true;
            }
            if seen.insert(f) {
                stack.extend(calls.get(&f).into_iter().flatten().copied());
            }
        }
        false
    };
    for (b, f) in unit.iter_mut() {
        f.recursive_arms = f
            .fragment
            .arm_bodies()
            .zip(&f.globals)
            .map(|(body, globals)| {
                body.is_some_and(|body| {
                    body.fragment.called_globals().iter().any(|(index, _)| {
                        globals
                            .iter()
                            .find(|(i, _)| i == index)
                            .is_some_and(|(_, g)| reaches(*g, *b))
                    })
                })
            })
            .collect();
    }
}

/// For each function of `unit`, the functions of `unit` its body calls with no
/// spliced operand.
fn collect_calls(h: &Hir, unit: &UnitFns, out: &mut FxHashMap<Binding, Vec<Binding>>) {
    let record = |b: Binding, init: &Hir, out: &mut FxHashMap<Binding, Vec<Binding>>| {
        if unit.contains_key(&b) {
            let mut callees = Vec::new();
            callees_of(init, unit, &mut callees);
            out.insert(b, callees);
        }
    };
    match &h.kind {
        HirKind::Let { bindings, .. } | HirKind::Letrec { bindings, .. } => {
            for (b, init) in bindings {
                record(*b, init, out);
            }
        }
        HirKind::Define { binding, value } => record(*binding, value, out),
        _ => {}
    }
    h.for_each_child(|c| collect_calls(c, unit, out));
}

fn callees_of(h: &Hir, unit: &UnitFns, out: &mut Vec<Binding>) {
    if let HirKind::Call { func, args, .. } = &h.kind {
        if let HirKind::Var(b) = &func.kind {
            if unit.contains_key(b) && !args.iter().any(|a| a.spliced) {
                out.push(*b);
            }
        }
    }
    h.for_each_child(|c| callees_of(c, unit, out));
}
