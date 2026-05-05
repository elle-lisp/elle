//! Type-aware signal narrowing and re-propagation.
//!
//! After type inference, strips SIG_ERROR from calls to known primitives
//! when argument types prove the error path is unreachable. Then
//! recomputes parent signals bottom-up so narrowed leaves propagate.

use super::arena::BindingArena;
use super::binding::Binding;
use super::expr::{CallArg, Hir, HirId, HirKind};
use super::types::{TyId, TypeInterner};
use crate::signals::Signal;
use crate::value::fiber::SignalBits;

use std::collections::HashMap;

/// Strip SIG_ERROR from calls to known primitives when argument types
/// prove the error path is unreachable.
pub(super) fn narrow_signals(
    hir: &mut Hir,
    interner: &TypeInterner,
    arena: &BindingArena,
    symbol_names: &HashMap<u32, String>,
    hir_types: &HashMap<HirId, TyId>,
    binding_min_length: &HashMap<Binding, usize>,
) {
    // Try to narrow this node
    if let HirKind::Call { func, args, .. } = &hir.kind {
        let callee_binding = super::typeinfer::unwrap_callee_binding(func);
        if let Some(callee_binding) = callee_binding {
            let callee_sym = arena.get(callee_binding).name;
            if let Some(name) = symbol_names.get(&callee_sym.0) {
                let arg_tys: Vec<TyId> = args
                    .iter()
                    .map(|a| {
                        hir_types
                            .get(&a.expr.id)
                            .copied()
                            .unwrap_or(TypeInterner::TOP)
                    })
                    .collect();
                if should_narrow_error(name, &arg_tys, args, interner, arena, binding_min_length) {
                    let sig_error = SignalBits::from_bit(0); // SIG_ERROR = bit 0
                    hir.signal.bits = hir.signal.bits.subtract(sig_error);
                }
            }
        }
    }

    // Recurse into children
    hir.for_each_child_mut(|child| {
        narrow_signals(
            child,
            interner,
            arena,
            symbol_names,
            hir_types,
            binding_min_length,
        );
    });
}

/// Determine if a call's SIG_ERROR can be stripped based on argument types.
fn should_narrow_error(
    name: &str,
    arg_tys: &[TyId],
    args: &[CallArg],
    interner: &TypeInterner,
    _arena: &BindingArena,
    binding_min_length: &HashMap<Binding, usize>,
) -> bool {
    match name {
        // Type predicates: never error
        "string?" | "int?" | "integer?" | "float?" | "number?" | "nil?" | "boolean?"
        | "keyword?" | "symbol?" | "pair?" | "list?" | "array?" | "struct?" | "bytes?"
        | "even?" | "odd?" | "closure?" | "fiber?" | "box?" | "ptr?" | "pointer?" => true,

        // type: never errors
        "type" => true,

        // empty?: never errors
        "empty?" => true,

        // string: all args must be stringifiable
        "string" => arg_tys.iter().all(|t| interner.is_stringifiable(*t)),

        // put on MutableStruct with keyword key
        "put" => {
            if arg_tys.len() >= 2 {
                let target = arg_tys[0];
                if target == TypeInterner::MUTABLE_STRUCT && arg_tys[1] == TypeInterner::KEYWORD {
                    return true;
                }
                // put on MutableArray with literal int index in bounds
                if target == TypeInterner::MUTABLE_ARRAY
                    && arg_tys.len() >= 3
                    && arg_tys[1] == TypeInterner::INT
                {
                    if let Some(idx) = extract_literal_int(&args[1].expr) {
                        if let Some(min_len) =
                            extract_target_min_length(&args[0].expr, binding_min_length)
                        {
                            if idx >= 0 && (idx as usize) < min_len {
                                return true;
                            }
                        }
                    }
                }
            }
            false
        }

        // push on MutableArray
        "push" => arg_tys
            .first()
            .is_some_and(|t| *t == TypeInterner::MUTABLE_ARRAY),

        // abs, floor, ceil, round: arg0 must be Number
        "abs" | "floor" | "ceil" | "round" => arg_tys
            .first()
            .is_some_and(|t| interner.subtype(*t, TypeInterner::NUMBER)),

        // has?: arg0 must be struct-like
        "has?" => arg_tys.first().is_some_and(|t| interner.is_struct(*t)),

        // length: arg0 is not Top (works on strings, arrays, lists)
        "length" => arg_tys.first().is_some_and(|t| *t != TypeInterner::TOP),

        // string/contains?: both args must be String
        "string/contains?" | "string-contains?" => {
            arg_tys.len() >= 2
                && arg_tys[0] == TypeInterner::STRING
                && arg_tys[1] == TypeInterner::STRING
        }

        // number->string: arg0 must be Number
        "number->string" => arg_tys
            .first()
            .is_some_and(|t| interner.subtype(*t, TypeInterner::NUMBER)),

        _ => false,
    }
}

fn extract_literal_int(hir: &Hir) -> Option<i64> {
    match &hir.kind {
        HirKind::Int(n) => Some(*n),
        _ => None,
    }
}

fn extract_target_min_length(
    hir: &Hir,
    binding_min_length: &HashMap<Binding, usize>,
) -> Option<usize> {
    match &hir.kind {
        HirKind::Var(b) => binding_min_length.get(b).copied(),
        HirKind::DerefCell { cell } => {
            if let HirKind::Var(b) = &cell.kind {
                binding_min_length.get(b).copied()
            } else {
                None
            }
        }
        _ => None,
    }
}

// ── Signal re-propagation ────────────────────────────────────────────

/// Recompute each node's signal bottom-up from its children.
/// Picks up both intrinsic rewrites (silent) and narrowed calls.
pub(super) fn repropagate_signals(hir: &mut Hir) {
    match &mut hir.kind {
        // Lambda: separate signal scope — recurse inside but don't propagate
        HirKind::Lambda { body, .. } => {
            repropagate_signals(body);
        }

        // Call: preserve narrowed signal, combine with child signals
        HirKind::Call { func, args, .. } => {
            repropagate_signals(func);
            for arg in args.iter_mut() {
                repropagate_signals(&mut arg.expr);
            }
            let mut sig = hir.signal;
            sig = sig.combine(func.signal);
            for arg in args.iter() {
                sig = sig.combine(arg.expr.signal);
            }
            hir.signal = sig;
        }

        // Emit: preserve emitted signal bits, just recurse
        HirKind::Emit { value, .. } => {
            repropagate_signals(value);
        }

        // Eval: always Yields — recurse but don't narrow
        HirKind::Eval { expr, env } => {
            repropagate_signals(expr);
            repropagate_signals(env);
        }

        // Intrinsic: already silent — just recurse
        HirKind::Intrinsic { args, .. } => {
            for arg in args.iter_mut() {
                repropagate_signals(arg);
            }
        }

        // Everything else: recurse, then combine child signals
        _ => {
            let mut sig = Signal::silent();
            hir.for_each_child_mut(|child| {
                repropagate_signals(child);
                sig = sig.combine(child.signal);
            });
            hir.signal = sig;
        }
    }
}
