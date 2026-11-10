// audited: 2026-10-21
//! One graft: the arm a call's operand count selects, its operands bound in order, the body in the call's place.
//!
//! docs/impl/graft.md

use super::super::contract::has_obligation;
use super::{Contribution, Grafter, Operand, UnitFn};
use crate::hir::arena::BindingScope;
use crate::hir::binding::Binding;
use crate::hir::expr::{CallArg, Hir, HirId, HirKind};
use crate::hir::fragment::ArmBody;
use crate::hir::types::{TyId, TypeInterner};
use crate::syntax::Span;
use crate::value::SymbolId;
use rustc_hash::FxHashMap;
use std::rc::Rc;

impl<'a> Grafter<'a> {
    /// The body that replaces `call`, or `None` to keep the call.
    pub(super) fn graft(&mut self, call: &mut Hir) -> Option<Hir> {
        let HirKind::Call { func, args, .. } = &call.kind else {
            return None;
        };
        if args.iter().any(|a| a.spliced) {
            return None;
        }
        let operand_count = args.len();
        let (fragment, own) = self.callee(func)?;
        let identity = Rc::as_ptr(&fragment);
        if self.grafting.contains(&identity) {
            return None;
        }
        let selected = fragment.select(operand_count)?;
        let arm = selected.body;
        let recursive = match own {
            Some(f) => f.recursive_arms[selected.arm],
            None => arm.recursive,
        };
        if recursive || arm.fragment.body_signal().propagates != 0 {
            return None;
        }
        // Where nothing live reaches, the operands prove nothing, so a body
        // with an op to prove would be rejected where the call compiles.
        if self.unreached > 0 && arm.fragment.holds_intrinsic_where(has_obligation) {
            return None;
        }
        let extras = operand_count - fragment.fixed();
        let list = self.prim_by_name.get(&SymbolId::of("list")).copied();
        if list.is_none() && extras > 0 && (arm.rest.is_some() || arm.tail.is_some()) {
            return None;
        }

        // Resolve every global before anything is minted, so a declined graft
        // leaves the arena as it found it.
        let globals: FxHashMap<u32, Binding> = match own {
            Some(f) => {
                let closed = &f.globals[selected.arm];
                if !closed
                    .iter()
                    .all(|(_, b)| self.scope.reaches(*b, self.arena))
                {
                    return None;
                }
                closed.iter().copied().collect()
            }
            None => {
                if !arm.fragment.globals_resolve(&self.prim_by_name) {
                    return None;
                }
                FxHashMap::default()
            }
        };
        // A body of this unit keeps the spans it was written with. Another
        // unit's spans may name files this process never interned, so its
        // body takes the call's.
        let rebuilt_span = match own {
            Some(_) => None,
            None => Some(call.span),
        };
        let prim_by_name = &self.prim_by_name;
        let (host, mut body) =
            arm.fragment
                .graft_with(self.arena, rebuilt_span, |index, name| match own {
                    Some(_) => globals.get(&index).copied(),
                    None => prim_by_name.get(&name).copied(),
                })?;

        let span = call.span;
        let HirKind::Call { args, .. } = std::mem::replace(&mut call.kind, HirKind::Nil) else {
            unreachable!("matched a call above");
        };
        let operands: Vec<Hir> = args.into_iter().map(|a: CallArg| a.expr).collect();
        let mut binder = Binder {
            host: &host,
            span,
            list,
            read: reads(&body),
            lets: Vec::new(),
            direct: FxHashMap::default(),
            contributions: Vec::new(),
        };
        self.bind_operands(arm, own, operands, &mut binder);
        let occurrences = binder.substitute(&mut body);
        for (param, def_param, operand) in binder.contributions {
            let operand = match operand {
                Operand::Occurrences(_, b) => {
                    Operand::Occurrences(occurrences.get(&param).cloned().unwrap_or_default(), b)
                }
                other => other,
            };
            let function = own.expect("contributions are recorded for this unit's functions");
            self.record.push(Contribution {
                function: function.binding,
                param: def_param,
                operand,
            });
        }

        // What the body calls grafts too, this function excluded.
        self.grafting.push(identity);
        self.walk(&mut body);
        self.grafting.pop();

        if binder.lets.is_empty() {
            return Some(body);
        }
        Some(Hir::new(
            HirKind::Let {
                bindings: binder.lets,
                body: Box::new(body),
            },
            span,
            call.signal,
        ))
    }

    /// Bind each operand to what receives it — the fixed parameters, then the
    /// extra operands — in the order they are written.
    fn bind_operands(
        &mut self,
        arm: &ArmBody,
        own: Option<&UnitFn>,
        operands: Vec<Hir>,
        binder: &mut Binder<'_>,
    ) {
        // Each operand is judged against the operands after it: a later one
        // is evaluated before the body reads an operand written in place.
        let direct: Vec<bool> = (0..operands.len())
            .map(|i| self.is_direct(&operands[i], &operands[i + 1..]))
            .collect();
        let mut operands = operands.into_iter().zip(direct);
        for (i, &index) in arm.params.iter().enumerate() {
            let (operand, direct) = operands.next().expect("select checked the operand count");
            let param = binder.host[index as usize];
            let read = if direct && !self.arena.get(param).declared_numeric {
                let read = match &operand.kind {
                    HirKind::Var(b) => Operand::Occurrences(Vec::new(), *b),
                    _ => Operand::Type(literal_type(&operand)),
                };
                binder.direct.insert(param, operand);
                read
            } else {
                self.rekind(param);
                binder.lets.push((param, operand));
                Operand::Local(param)
            };
            if let Some(f) = own {
                binder.contributions.push((param, f.params[i], read));
            }
        }

        // The extra operands, each evaluated once, in order, before the body.
        let extras: Vec<Hir> = operands
            .map(|(op, direct)| {
                if direct {
                    return op;
                }
                let temp = self.arena.gensym();
                self.arena.get_mut(temp).is_immutable = true;
                binder.lets.push((temp, op));
                Hir::silent(HirKind::Var(temp), binder.span)
            })
            .collect();
        if let Some(index) = arm.rest {
            let rest = binder.host[index as usize];
            self.bind_list(binder, rest, &extras);
            if let Some(def_rest) = own.and_then(|f| f.rest) {
                let shape = if extras.is_empty() {
                    TypeInterner::EMPTY_LIST
                } else {
                    TypeInterner::PAIR
                };
                binder
                    .contributions
                    .push((rest, def_rest, Operand::Type(shape)));
            }
        }
        for (element, extra) in arm.elements.iter().zip(&extras) {
            if let Some(index) = element {
                binder
                    .direct
                    .insert(binder.host[*index as usize], extra.clone());
            }
        }
        if let Some(index) = arm.tail {
            let tail = binder.host[index as usize];
            let from = arm.elements.len().min(extras.len());
            self.bind_list(binder, tail, &extras[from..]);
        }
    }

    /// A parameter the graft binds with a `let` is a local of the caller, and a
    /// compiler temporary there: no name the user wrote stands at that place.
    fn rekind(&mut self, b: Binding) {
        let inner = self.arena.get_mut(b);
        inner.scope = BindingScope::Local;
        inner.is_immutable = true;
        inner.is_synthetic = true;
    }

    /// Bind `b` to the list of `items`: the empty list in place, or a `let` of
    /// a call to `list`.
    fn bind_list(&mut self, binder: &mut Binder<'_>, b: Binding, items: &[Hir]) {
        if !binder.read.contains(&b) {
            return;
        }
        if items.is_empty() {
            binder
                .direct
                .insert(b, Hir::silent(HirKind::EmptyList, binder.span));
            return;
        }
        let list = binder
            .list
            .expect("graft checked that `list` resolves before it built a list");
        self.rekind(b);
        let call = Hir::silent(
            HirKind::Call {
                func: Box::new(Hir::silent(HirKind::Var(list), binder.span)),
                args: items
                    .iter()
                    .map(|e| CallArg {
                        expr: fresh(e),
                        spliced: false,
                    })
                    .collect(),
                is_tail: false,
            },
            binder.span,
        );
        binder.lets.push((b, call));
    }
}

/// What one graft accumulates while it binds the operands.
struct Binder<'h> {
    host: &'h [Binding],
    span: Span,
    list: Option<Binding>,
    /// The `let` that evaluates the bound operands, in the order written.
    lets: Vec<(Binding, Hir)>,
    /// The bindings the grafted body reads. A list nothing reads is not built.
    read: rustc_hash::FxHashSet<Binding>,
    /// The operands written in place of a binding the body names.
    direct: FxHashMap<Binding, Hir>,
    /// Per parameter of a function this unit defines: the host binding that
    /// stands for it, the parameter itself, and where its operand is read.
    contributions: Vec<(Binding, Binding, Operand)>,
}

impl Binder<'_> {
    /// Write each in-place operand over every occurrence of its binding,
    /// answering the ids of the occurrences written for each binding.
    fn substitute(&self, body: &mut Hir) -> FxHashMap<Binding, Vec<HirId>> {
        let mut written: FxHashMap<Binding, Vec<HirId>> = FxHashMap::default();
        substitute(body, &self.direct, &mut written);
        written
    }
}

fn substitute(
    h: &mut Hir,
    direct: &FxHashMap<Binding, Hir>,
    written: &mut FxHashMap<Binding, Vec<HirId>>,
) {
    if let HirKind::Var(b) = &h.kind {
        if let Some(op) = direct.get(b) {
            let b = *b;
            *h = fresh(op);
            written.entry(b).or_default().push(h.id);
        }
        return;
    }
    h.for_each_child_mut(|c| substitute(c, direct, written));
}

/// Every binding a body reads.
fn reads(h: &Hir) -> rustc_hash::FxHashSet<Binding> {
    fn walk(h: &Hir, out: &mut rustc_hash::FxHashSet<Binding>) {
        if let HirKind::Var(b) = &h.kind {
            out.insert(*b);
        }
        h.for_each_child(|c| walk(c, out));
    }
    let mut out = rustc_hash::FxHashSet::default();
    walk(h, &mut out);
    out
}

/// A copy of a leaf operand with an id of its own.
fn fresh(op: &Hir) -> Hir {
    Hir::new(op.kind.clone(), op.span, op.signal)
}

/// The type an immediate literal operand has.
fn literal_type(h: &Hir) -> TyId {
    match &h.kind {
        HirKind::Nil => TypeInterner::NIL,
        HirKind::EmptyList => TypeInterner::EMPTY_LIST,
        HirKind::Bool(_) => TypeInterner::BOOL,
        HirKind::Int(_) => TypeInterner::INT,
        HirKind::Float(_) => TypeInterner::FLOAT,
        HirKind::Keyword(_) => TypeInterner::KEYWORD,
        _ => TypeInterner::TOP,
    }
}
