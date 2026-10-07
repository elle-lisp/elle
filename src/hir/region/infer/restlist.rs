// audited: 2026-10-06
//! The rest-list gate: which variadic lambdas build their `&` rest list in one region.
//!
//! docs/impl/region/restlist.md

use super::*;
use crate::hir::expr::IntrinsicOp;
use crate::hir::VarargKind;
use crate::primitives::def::RegionEffect;
use crate::value::SymbolId;
use rustc_hash::FxHashSet;

/// Every lambda in `hir` whose `&` rest list no cell of can outlive its head:
/// the parameter is never reassigned, no nested lambda reaches it, and every
/// reference to it reads the list without taking a cell out.
pub(super) fn one_region_rest_lists(
    hir: &Hir,
    arena: &BindingArena,
    call_class: &CallClassification,
) -> FxHashSet<HirId> {
    let gate = Gate { arena, call_class };
    let mut admitted = FxHashSet::default();
    gate.collect(hir, &mut admitted);
    admitted
}

struct Gate<'a> {
    arena: &'a BindingArena,
    call_class: &'a CallClassification,
}

impl Gate<'_> {
    fn collect(&self, hir: &Hir, admitted: &mut FxHashSet<HirId>) {
        if let HirKind::Lambda {
            rest_param: Some(rest),
            vararg_kind: VarargKind::List,
            body,
            ..
        } = &hir.kind
        {
            let b = self.arena.get(*rest);
            if b.is_immutable && !b.is_mutated && self.only_reads(body, *rest) {
                admitted.insert(hir.id);
            }
        }
        hir.for_each_child(|c| self.collect(c, admitted));
    }

    /// Does every reference to `rest` inside `hir` sit in a position that reads
    /// the list and keeps no cell of it? A reference this walk reaches as a
    /// bare `Var` sits in no such position.
    fn only_reads(&self, hir: &Hir, rest: Binding) -> bool {
        match &hir.kind {
            HirKind::Var(b) => *b != rest,
            HirKind::Lambda { captures, body, .. } => {
                !captures.iter().any(|c| c.binding == rest) && !mentions(body, rest)
            }
            HirKind::Call { func, args, .. } => {
                let reads = self.callee_reads_its_arguments(func, args.len());
                args.iter().all(|a| {
                    if is_var(&a.expr, rest) {
                        a.spliced || reads
                    } else {
                        self.only_reads(&a.expr, rest)
                    }
                }) && self.only_reads(func, rest)
            }
            HirKind::Intrinsic { op, args } if needs_no_proof(*op) => args
                .iter()
                .all(|a| is_var(a, rest) || self.only_reads(a, rest)),
            _ => {
                let mut ok = true;
                hir.for_each_child(|c| ok = ok && self.only_reads(c, rest));
                ok
            }
        }
    }

    /// Is `func` a native primitive that reads its arguments and answers no
    /// cell of them: one declaring `Immediate`, or `first` or `second` called
    /// on one argument, whose built-in method answers an element? A binding of
    /// that name that is not the native, such as a closure shadowing it or the
    /// standard library's closure over it, is refused.
    fn callee_reads_its_arguments(&self, func: &Hir, nargs: usize) -> bool {
        let HirKind::Var(f) = &func.kind else {
            return false;
        };
        let b = self.arena.get(*f);
        if !(b.is_primitive && b.is_native_fn && b.is_immutable && !b.is_mutated) {
            return false;
        }
        self.call_class.effects.get(&b.name) == Some(&RegionEffect::Immediate)
            || (nargs == 1 && (b.name == SymbolId::of("first") || b.name == SymbolId::of("second")))
    }
}

/// The intrinsics total on every value, which answer an immediate and keep no
/// operand (docs/intrinsics.md).
fn needs_no_proof(op: IntrinsicOp) -> bool {
    use IntrinsicOp::*;
    matches!(
        op,
        Eq | Ne
            | Identical
            | Not
            | TypeOf
            | IsNil
            | IsEmpty
            | IsBool
            | IsInt
            | IsFloat
            | IsString
            | IsKeyword
            | IsSymbol
            | IsPair
            | IsArray
            | IsStruct
            | IsSet
            | IsBytes
            | IsBox
            | IsClosure
            | IsFiber
    )
}

fn is_var(hir: &Hir, b: Binding) -> bool {
    matches!(hir.kind, HirKind::Var(v) if v == b)
}

/// Does any node of `hir` reference `b`?
fn mentions(hir: &Hir, b: Binding) -> bool {
    if is_var(hir, b) {
        return true;
    }
    if let HirKind::Lambda { captures, .. } = &hir.kind {
        if captures.iter().any(|c| c.binding == b) {
            return true;
        }
    }
    let mut found = false;
    hir.for_each_child(|c| found = found || mentions(c, b));
    found
}
