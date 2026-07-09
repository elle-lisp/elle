//! Discharging a single call-position site: match its op's contract row
//! against the inferred operand types (and the nonzero-divisor facts), and
//! either lower silently or reject with a diagnostic.

use super::*;

/// The inferred type of an operand occurrence (post-narrowing, per occurrence).
fn ty_of(h: &Hir, hir_types: &HashMap<HirId, TyId>) -> TyId {
    hir_types.get(&h.id).copied().unwrap_or(TypeInterner::TOP)
}

/// Is this operand provably ≠ 0 on the current path?
fn proven_nonzero(h: &Hir, env: &NonzeroEnv) -> bool {
    match &h.kind {
        HirKind::Int(n) => *n != 0,
        HirKind::Float(f) => *f != 0.0,
        HirKind::Var(b) => env.proves(*b),
        HirKind::DerefCell { cell } => {
            matches!(&cell.kind, HirKind::Var(b) if env.proves(*b))
        }
        _ => false,
    }
}

fn reject(site: &Hir, op: IntrinsicOp, detail: String) -> Result<(), String> {
    Err(format!(
        "{}: {}: {} — a %-intrinsic in call position must prove its operands \
         (docs/intrinsics.md § The contract: prove or reject); use the stdlib \
         wrapper for dynamic types",
        site.span,
        op.name(),
        detail,
    ))
}

pub(super) fn check_op(
    op: IntrinsicOp,
    args: &[&Hir],
    site: &Hir,
    hir_types: &HashMap<HirId, TyId>,
    interner: &TypeInterner,
    env: &NonzeroEnv,
) -> Result<(), String> {
    let ty = |i: usize| ty_of(args[i], hir_types);
    let all_subtype = |bound: TyId| -> Option<usize> {
        (0..args.len()).find(|&i| !interner.subtype(ty(i), bound))
    };
    match op_contract(op) {
        Contract::Total => Ok(()),
        Contract::Numbers => match all_subtype(TypeInterner::NUMBER) {
            None => Ok(()),
            Some(i) => reject(
                site,
                op,
                format!(
                    "operand {} is not a proven number (inferred: {})",
                    i + 1,
                    TypeInterner::describe(ty(i))
                ),
            ),
        },
        Contract::DivFamily => {
            if let Some(i) = all_subtype(TypeInterner::NUMBER) {
                return reject(
                    site,
                    op,
                    format!(
                        "operand {} is not a proven number (inferred: {})",
                        i + 1,
                        TypeInterner::describe(ty(i))
                    ),
                );
            }
            // The div opcodes' one undefined zero case is int÷int: with either
            // operand a proven Float the whole computation is IEEE-defined
            // (±inf/NaN) on every tier, so the nonzero obligation applies only
            // when no operand is proven Float.
            let any_float = (0..args.len()).any(|i| ty(i) == TypeInterner::FLOAT);
            let divisor = args[args.len() - 1];
            if any_float || proven_nonzero(divisor, env) {
                Ok(())
            } else {
                reject(
                    site,
                    op,
                    "the divisor is not provably nonzero (guard it: \
                     `(when (%eq d 0) (error …))` proves `d` on the fall-through; \
                     a proven-float divisor is exempt — IEEE semantics)"
                        .to_string(),
                )
            }
        }
        Contract::Ints => match all_subtype(TypeInterner::INT) {
            None => Ok(()),
            Some(i) => reject(
                site,
                op,
                format!(
                    "operand {} is not a proven int (inferred: {})",
                    i + 1,
                    TypeInterner::describe(ty(i))
                ),
            ),
        },
        Contract::Ordered => {
            let (a, b) = (ty(0), ty(1));
            let numbers = interner.subtype(a, TypeInterner::NUMBER)
                && interner.subtype(b, TypeInterner::NUMBER);
            let strings = a == TypeInterner::STRING && b == TypeInterner::STRING;
            let keywords = a == TypeInterner::KEYWORD && b == TypeInterner::KEYWORD;
            if numbers || strings || keywords {
                Ok(())
            } else {
                reject(
                    site,
                    op,
                    format!(
                        "operands are not one proven comparable family — both \
                         numbers, strings, or keywords (inferred: {} and {})",
                        TypeInterner::describe(a),
                        TypeInterner::describe(b)
                    ),
                )
            }
        }
        Contract::PairArg => {
            if ty(0) == TypeInterner::PAIR {
                Ok(())
            } else {
                reject(
                    site,
                    op,
                    format!(
                        "operand is not a proven pair (inferred: {})",
                        TypeInterner::describe(ty(0))
                    ),
                )
            }
        }
        Contract::Container { families, what } => {
            if families.contains(&ty(0)) {
                Ok(())
            } else {
                reject(
                    site,
                    op,
                    format!(
                        "container argument is not a statically-proven {} \
                         (inferred: {})",
                        what,
                        TypeInterner::describe(ty(0))
                    ),
                )
            }
        }
        Contract::Get => {
            let c = ty(0);
            let key = ty(1);
            if ARRAY_FAMILY.contains(&c) || c == TypeInterner::STRING {
                if interner.subtype(key, TypeInterner::INT) {
                    Ok(())
                } else {
                    reject(
                        site,
                        op,
                        format!(
                            "an indexed container needs a proven int index \
                             (inferred: {})",
                            TypeInterner::describe(key)
                        ),
                    )
                }
            } else if STRUCT_FAMILY.contains(&c) {
                // The key must be proven hashable (`TableKey::from_value`'s
                // domain): a float or mutable container can never BE a struct
                // key, and the surface `get` reports that as a :type-error —
                // so the silent opcode may only lower over a proven key.
                const HASHABLE_KEYS: &[TyId] = &[
                    TypeInterner::NIL,
                    TypeInterner::BOOL,
                    TypeInterner::INT,
                    TypeInterner::KEYWORD,
                    TypeInterner::SYMBOL,
                    TypeInterner::STRING,
                    TypeInterner::ARRAY,
                ];
                if HASHABLE_KEYS.contains(&key) {
                    Ok(())
                } else {
                    reject(
                        site,
                        op,
                        format!(
                            "a struct key must be a proven hashable                              (nil/bool/int/keyword/symbol/string/array;                              inferred: {})",
                            TypeInterner::describe(key)
                        ),
                    )
                }
            } else {
                reject(
                    site,
                    op,
                    format!(
                        "container argument is not a statically-proven array, \
                         struct, or string (inferred: {})",
                        TypeInterner::describe(c)
                    ),
                )
            }
        }
    }
}
