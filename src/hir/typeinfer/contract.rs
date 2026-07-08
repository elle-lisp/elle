//! The intrinsic operand proofs: prove-or-reject, per docs/intrinsics.md.
//!
//! A `%`-intrinsic in call position is a compile-time type-checked request for
//! the fast lowering. Each op carries a soundness contract derived from what
//! its lowering actually trusts (the opcode handlers in `src/vm/` — wrong
//! operands there compute garbage, never a catchable error), and every
//! call-position use must discharge it from the inferred operand types:
//! proven ⇒ the site lowers and is silent; provably wrong **or unprovable** ⇒
//! compile error. Value-position uses are the registered `NativeFn`, which
//! validates at runtime — nothing to prove there.
//!
//! Call-position sites appear in two HIR shapes, checked identically: the
//! opcode `Intrinsic` node (non-storing ops) and the native funnel `Call`
//! whose callee is the `%`-named NativeFn (storing/copying ops — the
//! escape-correct region path).
//!
//! The div family (`%div`/`%rem`/`%mod`) carries a **value** obligation on top
//! of the type: the divisor must be provably nonzero (integer division by
//! zero has no silent total semantics). Nonzero facts flow like the type
//! narrowing does: a nonzero literal, a binding initialized from one, or a
//! diverging zero guard (`(when (%eq d 0) (error …))`) upstream of the site.
//! Reassignment invalidates the fact.

use super::*;
use std::collections::HashSet;

/// The type set a container op's first operand may inhabit.
const ARRAY_FAMILY: &[TyId] = &[TypeInterner::ARRAY, TypeInterner::MUTABLE_ARRAY];
const STRUCT_FAMILY: &[TyId] = &[TypeInterner::STRUCT, TypeInterner::MUTABLE_STRUCT];
const STRING_FAMILY: &[TyId] = &[TypeInterner::STRING, TypeInterner::MUTABLE_STRING];
const BYTES_FAMILY: &[TyId] = &[TypeInterner::BYTES, TypeInterner::MUTABLE_BYTES];

/// What an op's lowering trusts its operands to satisfy. One row per shape;
/// `op_contract` maps every `IntrinsicOp` onto a row (exhaustively — a new op
/// fails the build until it declares its contract).
enum Contract {
    /// Total on every value (equality, identity, truthiness, predicates,
    /// `%type-of`, `%pair`, and the pass-through `%freeze`/`%thaw`).
    Total,
    /// Every operand ⊑ Number (wrapping arithmetic, unary negate, conversions).
    Numbers,
    /// Every operand ⊑ Number AND the divisor (last operand) provably nonzero.
    DivFamily,
    /// Every operand ⊑ Int (bitwise and shifts).
    Ints,
    /// Both operands in one comparable family: Number/Number, string/string,
    /// keyword/keyword (the ordering opcodes compare exactly these).
    Ordered,
    /// First operand a pair (`%first`/`%rest` — their opcodes trust the cell).
    PairArg,
    /// First operand's type ∈ the listed set, described as `what`.
    Container {
        families: &'static [TyId],
        what: &'static str,
    },
    /// `%get`: container-dependent key legality (array/string index ⊑ Int;
    /// struct keys proven hashable — the surface `get` raises :type-error for
    /// an unhashable key, and the opcode's unreachable-by-proof path panics).
    Get,
}

/// The lengths (`%length`) domain: every container, plus lists (pair chains,
/// the empty list) and nil — exactly the cases its opcode handles.
const LENGTH_DOMAIN: &[TyId] = &[
    TypeInterner::ARRAY,
    TypeInterner::MUTABLE_ARRAY,
    TypeInterner::STRUCT,
    TypeInterner::MUTABLE_STRUCT,
    TypeInterner::SET,
    TypeInterner::MUTABLE_SET,
    TypeInterner::STRING,
    TypeInterner::MUTABLE_STRING,
    TypeInterner::BYTES,
    TypeInterner::MUTABLE_BYTES,
    TypeInterner::PAIR,
    TypeInterner::EMPTY_LIST,
    TypeInterner::NIL,
];

const HAS_DOMAIN: &[TyId] = &[
    TypeInterner::STRUCT,
    TypeInterner::MUTABLE_STRUCT,
    TypeInterner::SET,
    TypeInterner::MUTABLE_SET,
    TypeInterner::STRING,
    TypeInterner::MUTABLE_STRING,
];

const PUT_DOMAIN: &[TyId] = &[
    TypeInterner::STRUCT,
    TypeInterner::MUTABLE_STRUCT,
    TypeInterner::ARRAY,
    TypeInterner::MUTABLE_ARRAY,
];

const DEL_DOMAIN: &[TyId] = &[
    TypeInterner::STRUCT,
    TypeInterner::MUTABLE_STRUCT,
    TypeInterner::SET,
    TypeInterner::MUTABLE_SET,
];

const POP_DOMAIN: &[TyId] = &[TypeInterner::MUTABLE_ARRAY];

fn op_contract(op: IntrinsicOp) -> Contract {
    use IntrinsicOp::*;
    match op {
        Add | Sub | Mul => Contract::Numbers,
        Div | Rem | Mod => Contract::DivFamily,
        Int | Float => Contract::Numbers,
        BitAnd | BitOr | BitXor | BitNot | Shl | Shr => Contract::Ints,
        Lt | Gt | Le | Ge => Contract::Ordered,
        Eq | Ne | Identical | Not | Pair | TypeOf => Contract::Total,
        IsNil | IsEmpty | IsBool | IsInt | IsFloat | IsString | IsKeyword | IsSymbol | IsPair
        | IsArray | IsStruct | IsSet | IsBytes | IsBox | IsClosure | IsFiber => Contract::Total,
        First | Rest => Contract::PairArg,
        Length => Contract::Container {
            families: LENGTH_DOMAIN,
            what: "container, list, or nil",
        },
        Get => Contract::Get,
        Has => Contract::Container {
            families: HAS_DOMAIN,
            what: "struct, set, or string",
        },
        // The monomorphic put/push variants pin the family; mutability is the
        // runtime dispatch's business (both mutabilities are family-legal, the
        // same gate the monomorphization obligation always held).
        Put => Contract::Container {
            families: PUT_DOMAIN,
            what: "struct or array",
        },
        PutStruct | PutStructMut => Contract::Container {
            families: STRUCT_FAMILY,
            what: "struct",
        },
        PutArray | PutArrayMut => Contract::Container {
            families: ARRAY_FAMILY,
            what: "array",
        },
        Push | PushArray | PushArrayMut => Contract::Container {
            families: ARRAY_FAMILY,
            what: "array",
        },
        Del => Contract::Container {
            families: DEL_DOMAIN,
            what: "struct or set",
        },
        Pop => Contract::Container {
            families: POP_DOMAIN,
            what: "@array",
        },
        // The storing ops' compile gate owns the *container* (the operand the
        // region system and the opcode's dispatch trust); the pushed value's
        // legality is the funnel native's runtime validation, which signals
        // like any native (`prim_string_push` / `prim_bytes_push`).
        StringPush => Contract::Container {
            families: STRING_FAMILY,
            what: "string",
        },
        BytesPush => Contract::Container {
            families: BYTES_FAMILY,
            what: "bytes",
        },
        // Pass-throughs on already-right-mutability inputs, copies otherwise;
        // total on every value.
        Freeze | Thaw => Contract::Total,
    }
}

/// Nonzero-divisor flow environment: bindings proven ≠ 0 on the current path.
#[derive(Default, Clone)]
struct NonzeroEnv(HashSet<Binding>);

impl NonzeroEnv {
    fn apply(&mut self, facts: &[guard::Fact]) {
        for f in facts {
            if let guard::Fact::Nonzero(b) = f {
                self.0.insert(*b);
            }
        }
    }
    fn invalidate(&mut self, b: Binding) {
        self.0.remove(&b);
    }
    fn proves(&self, b: Binding) -> bool {
        self.0.contains(&b)
    }
}

/// Check every call-position `%`-intrinsic in the tree against its contract.
/// Walks in evaluation order, carrying the nonzero-divisor facts.
pub(super) fn check_intrinsic_operand_proofs(
    hir: &Hir,
    hir_types: &HashMap<HirId, TyId>,
    arena: &BindingArena,
    symbol_names: &HashMap<u32, String>,
) -> Result<(), String> {
    let interner = TypeInterner::new();
    let mut env = NonzeroEnv::default();
    walk(hir, hir_types, arena, symbol_names, &interner, &mut env)
}

fn walk(
    h: &Hir,
    hir_types: &HashMap<HirId, TyId>,
    arena: &BindingArena,
    symbol_names: &HashMap<u32, String>,
    interner: &TypeInterner,
    env: &mut NonzeroEnv,
) -> Result<(), String> {
    macro_rules! recurse {
        ($e:expr, $env:expr) => {
            walk($e, hir_types, arena, symbol_names, interner, $env)
        };
    }

    match &h.kind {
        HirKind::Intrinsic { op, args } => {
            for a in args {
                recurse!(a, env)?;
            }
            let arg_refs: Vec<&Hir> = args.iter().collect();
            check_op(*op, &arg_refs, h, hir_types, interner, env)
        }
        HirKind::Call { func, args, .. } => {
            recurse!(func, env)?;
            for a in args {
                recurse!(&a.expr, env)?;
            }
            // A call whose callee is the %-named NativeFn is the storing ops'
            // call-position form — same proof, native lowering. A spliced
            // argument list has no per-operand types to check; the native's
            // own runtime validation covers that (dynamic) shape.
            if let HirKind::Var(b) = &func.kind {
                if let Some(name) = symbol_names.get(&arena.get(*b).name.0) {
                    if name.starts_with('%') && !args.iter().any(|a| a.spliced) {
                        if let Some(op) = IntrinsicOp::from_name(name) {
                            let arg_refs: Vec<&Hir> = args.iter().map(|a| &a.expr).collect();
                            return check_op(op, &arg_refs, h, hir_types, interner, env);
                        }
                    }
                }
            }
            Ok(())
        }
        // Statement sequences: a diverging one-armed guard leaves its
        // fall-through facts standing for the rest of the sequence.
        HirKind::Begin(exprs) => {
            let saved = env.clone();
            for e in exprs {
                recurse!(e, env)?;
                env.apply(&guard::facts_after_statement(e, arena, symbol_names));
            }
            *env = saved;
            Ok(())
        }
        HirKind::Block { body, .. } => {
            let saved = env.clone();
            for e in body {
                recurse!(e, env)?;
                env.apply(&guard::facts_after_statement(e, arena, symbol_names));
            }
            *env = saved;
            Ok(())
        }
        HirKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            recurse!(cond, env)?;
            let facts = guard::cond_facts(cond, arena, symbol_names);
            let mut then_env = env.clone();
            then_env.apply(&facts.when_true);
            recurse!(then_branch, &mut then_env)?;
            let mut else_env = env.clone();
            else_env.apply(&facts.when_false);
            recurse!(else_branch, &mut else_env)?;
            // A diverging branch leaves the other branch's facts standing.
            if guard::diverges(then_branch) {
                env.apply(&facts.when_false);
            }
            if guard::diverges(else_branch) {
                env.apply(&facts.when_true);
            }
            Ok(())
        }
        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            // Sequential If chain: each body gets its test's truthy facts;
            // later clauses (and the else) know every earlier test was falsy.
            // The falsy accumulation is scoped to the Cond — after it, some
            // clause may have run, so none of the falsy facts hold outside.
            let mut running = env.clone();
            for (test, body) in clauses {
                recurse!(test, &mut running)?;
                let facts = guard::cond_facts(test, arena, symbol_names);
                let mut body_env = running.clone();
                body_env.apply(&facts.when_true);
                recurse!(body, &mut body_env)?;
                running.apply(&facts.when_false);
            }
            if let Some(els) = else_branch {
                recurse!(els, &mut running)?;
            }
            Ok(())
        }
        HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
            for (b, init) in bindings {
                recurse!(init, env)?;
                // A binding initialized from a nonzero literal is itself proven.
                match &init.kind {
                    HirKind::Int(n) if *n != 0 => env.0.insert(*b),
                    HirKind::Float(f) if *f != 0.0 => env.0.insert(*b),
                    _ => false,
                };
            }
            recurse!(body, env)
        }
        // Mutation invalidates a proven fact.
        HirKind::Assign { target, value }
        | HirKind::Define {
            binding: target,
            value,
        } => {
            recurse!(value, env)?;
            env.invalidate(*target);
            Ok(())
        }
        HirKind::SetCell { cell, value } => {
            recurse!(cell, env)?;
            recurse!(value, env)?;
            if let HirKind::Var(b) = &cell.kind {
                env.invalidate(*b);
            }
            Ok(())
        }
        // A loop body re-enters: any binding it mutates is unproven for the
        // whole body (the back edge would carry the mutated value into a use
        // textually before the mutation).
        HirKind::Loop { bindings, body } => {
            for (_, init) in bindings {
                recurse!(init, env)?;
            }
            let mut body_env = env.clone();
            for b in collect_mutated(body) {
                body_env.invalidate(b);
            }
            recurse!(body, &mut body_env)
        }
        // A lambda body runs at unknown times relative to the surrounding
        // flow; it starts with no path facts.
        HirKind::Lambda { body, .. } => {
            let mut fresh = NonzeroEnv::default();
            recurse!(body, &mut fresh)
        }
        HirKind::Match { value, arms } => {
            recurse!(value, env)?;
            for (_, arm_guard, arm_body) in arms {
                let mut arm_env = env.clone();
                if let Some(g) = arm_guard {
                    recurse!(g, &mut arm_env)?;
                }
                recurse!(arm_body, &mut arm_env)?;
            }
            Ok(())
        }
        _ => {
            let mut result = Ok(());
            h.for_each_child(|c| {
                if result.is_ok() {
                    result = recurse!(c, env);
                }
            });
            result
        }
    }
}

/// Bindings mutated anywhere inside `h` (Assign / Define / SetCell targets).
fn collect_mutated(h: &Hir) -> Vec<Binding> {
    let mut out = Vec::new();
    fn go(h: &Hir, out: &mut Vec<Binding>) {
        match &h.kind {
            HirKind::Assign { target, .. }
            | HirKind::Define {
                binding: target, ..
            } => {
                out.push(*target);
            }
            HirKind::SetCell { cell, .. } => {
                if let HirKind::Var(b) = &cell.kind {
                    out.push(*b);
                }
            }
            _ => {}
        }
        h.for_each_child(|c| go(c, out));
    }
    go(h, &mut out);
    out
}

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

fn check_op(
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
