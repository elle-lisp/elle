use super::super::*;
use crate::hir::pattern::{HirPattern, PatternLiteral};

/// The binding a `(type-of <var>)` match scrutinee discriminates, for match-arm
/// narrowing. Recognizes both the `type-of`/`type` callable and the `%type-of`
/// intrinsic; the subject must be a `Var` or `DerefCell { Var }` (a letrec/cell
/// read), mirroring `extract_type_guard`'s subject extraction.
///
/// ANF names the scrutinee, so `(match (type-of c) …)` reaches here as
/// `(match (let [t (type-of c)] t) …)` — the `value` is a `Let` whose body reads
/// the named call result, not the call itself. `unwrap_anf_let` follows that
/// binding back to the underlying expression before matching `type-of`.
pub(crate) fn typeof_subject_binding(
    value: &Hir,
    arena: &BindingArena,
    symbol_names: &HashMap<u32, String>,
    typeof_aliases: &HashMap<Binding, Binding>,
) -> Option<Binding> {
    let inner = unwrap_anf_let(value);
    if let Some(subj) = typeof_call_subject(inner, arena, symbol_names) {
        return Some(subj);
    }
    // The `(let [ta (type-of a)] (match ta …))` idiom: the scrutinee is a plain
    // immutable alias of an earlier `(type-of a)` rather than the inline call.
    // `collect_typeof_aliases` has resolved `ta → a`, so narrowing / dead-arm
    // pruning fires on `a` exactly as for the inline `(match (type-of a) …)`.
    typeof_aliases.get(&var_of(inner)?).copied()
}

/// The subject binding of a `(type-of x)` / `(%type-of x)` expression — `x` a
/// `Var` or `DerefCell{Var}` — else `None`. `h` must already be positioned at the
/// underlying expression (callers unwrap ANF/cell wrappers first).
pub(crate) fn typeof_call_subject(
    h: &Hir,
    arena: &BindingArena,
    symbol_names: &HashMap<u32, String>,
) -> Option<Binding> {
    let subject = match &h.kind {
        HirKind::Intrinsic {
            op: IntrinsicOp::TypeOf,
            args,
        } if args.len() == 1 => &args[0],
        HirKind::Call { func, args, .. } if args.len() == 1 => {
            let callee = unwrap_callee_binding(func)?;
            let name = symbol_names.get(&arena.get(callee).name.0)?;
            if name != "type-of" && name != "type" {
                return None;
            }
            &args[0].expr
        }
        _ => return None,
    };
    var_of(subject)
}

/// A `Var(b)` or `DerefCell { Var(b) }` reference's binding, else `None`.
pub(crate) fn var_of(h: &Hir) -> Option<Binding> {
    match &h.kind {
        HirKind::Var(b) => Some(*b),
        HirKind::DerefCell { cell } => match &cell.kind {
            HirKind::Var(b) => Some(*b),
            _ => None,
        },
        _ => None,
    }
}

/// Follow an ANF-introduced `(let [t EXPR] t)` wrapper to `EXPR`. ANF names a
/// non-trivial scrutinee, binding it once and returning the bound var; this
/// unwraps that single-use naming (iteratively, in case of nesting) so the
/// underlying `type-of` call is visible. A `Let` whose body is not one of its
/// own bindings is returned as-is.
pub(crate) fn unwrap_anf_let(h: &Hir) -> &Hir {
    let mut cur = h;
    while let HirKind::Let { bindings, body } = &cur.kind {
        let Some(b) = var_of(body) else { break };
        let Some((_, init)) = bindings.iter().find(|(bb, _)| *bb == b) else {
            break;
        };
        cur = init;
    }
    cur
}

/// The concrete type a keyword-literal arm pattern proves for a `(type-of x)`
/// scrutinee — the containers in both mutabilities, plus the scalar type
/// keywords (`:integer`/`:float`/…) so a scalar dispatch arm (the stdlib `/`'s
/// int-vs-float divisor split) proves its scrutinee too.
pub(crate) fn pattern_type_keyword(pat: &HirPattern) -> Option<TyId> {
    let HirPattern::Literal(PatternLiteral::Keyword(s)) = pat else {
        return None;
    };
    // The reader stores keyword patterns without the leading `:`; tolerate both.
    let s = s.strip_prefix(':').unwrap_or(s);
    match s {
        "@array" => Some(TypeInterner::MUTABLE_ARRAY),
        "@struct" => Some(TypeInterner::MUTABLE_STRUCT),
        "@string" => Some(TypeInterner::MUTABLE_STRING),
        "@bytes" => Some(TypeInterner::MUTABLE_BYTES),
        "array" => Some(TypeInterner::ARRAY),
        "struct" => Some(TypeInterner::STRUCT),
        "string" => Some(TypeInterner::STRING),
        "bytes" => Some(TypeInterner::BYTES),
        "@set" => Some(TypeInterner::MUTABLE_SET),
        "set" => Some(TypeInterner::SET),
        "integer" => Some(TypeInterner::INT),
        "float" => Some(TypeInterner::FLOAT),
        "boolean" => Some(TypeInterner::BOOL),
        "keyword" => Some(TypeInterner::KEYWORD),
        "symbol" => Some(TypeInterner::SYMBOL),
        "nil" => Some(TypeInterner::NIL),
        _ => None,
    }
}
