//! Bidirectional type inference and stdlib-to-intrinsic rewriting.
//!
//! Post-functionalize pass that:
//! 1. Infers types from literals, known return types, and type guards
//! 2. Propagates types through call sites (forward flow)
//! 3. Rewrites stdlib arithmetic/comparison calls to intrinsics when
//!    argument types prove ⊑ Number
//! 4. Updates signals for rewritten nodes (intrinsics are silent)
//! 5. Narrows signals on primitive calls with provably typed args
//!    (delegates to `narrow.rs`)
//! 6. Re-propagates signals bottom-up after narrowing
//!
//! The pass iterates to a fixed point: type refinements enable rewrites,
//! which change signals, which enable further refinements.

use super::arena::BindingArena;
use super::binding::Binding;
use super::expr::{Hir, HirId, HirKind, IntrinsicOp};
use super::types::{TyId, TypeInterner};
use crate::signals::Signal;
use crate::symbol::SymbolTable;

use std::collections::HashMap;

/// Result of type inference — currently just tracks whether the pass
/// found any immediates for region inference.
pub struct TypeInfo {
    pub hir_types: HashMap<HirId, TyId>,
}

/// Which stdlib function maps to which intrinsic, and what type constraint.
struct RewriteRule {
    op: IntrinsicOp,
    arity: (usize, usize),
    /// Required type for all arguments (None = always valid)
    constraint: Option<TyId>,
}

/// Build the rewrite table mapping function names to intrinsic rewrites.
fn build_rewrite_table() -> HashMap<&'static str, RewriteRule> {
    let mut table = HashMap::new();
    let number = Some(TypeInterner::NUMBER);

    let mut add =
        |name: &'static str, op: IntrinsicOp, arity: (usize, usize), constraint: Option<TyId>| {
            table.insert(
                name,
                RewriteRule {
                    op,
                    arity,
                    constraint,
                },
            );
        };

    // Arithmetic (require Number)
    // Note: / , rem, mod have division-by-zero checks in stdlib that %div/%rem/%mod bypass.
    // Only rewrite operations that are total on Number.
    add("+", IntrinsicOp::Add, (2, 2), number);
    add("-", IntrinsicOp::Sub, (1, 2), number);
    add("*", IntrinsicOp::Mul, (2, 2), number);

    // Comparison (require Number — stdlib also accepts strings/keywords
    // but we only rewrite when we know it's numeric)
    add("<", IntrinsicOp::Lt, (2, 2), number);
    add(">", IntrinsicOp::Gt, (2, 2), number);
    add("<=", IntrinsicOp::Le, (2, 2), number);
    add(">=", IntrinsicOp::Ge, (2, 2), number);

    // Equality (always valid)
    add("=", IntrinsicOp::Eq, (2, 2), None);

    // Logical (always valid)
    add("not", IntrinsicOp::Not, (1, 1), None);

    table
}

const MAX_ITERS: usize = 10;

/// Run type inference and stdlib-to-intrinsic rewriting on functionalized HIR.
pub fn infer_and_rewrite(hir: &mut Hir, arena: &BindingArena, symbols: &SymbolTable) -> TypeInfo {
    // When --checked-intrinsics is active, intrinsics route through
    // type-checked NativeFn paths. Don't rewrite to bypass those checks.
    if crate::config::get().checked_intrinsics {
        return TypeInfo {
            hir_types: HashMap::new(),
        };
    }

    let interner = TypeInterner::new();
    let rewrite_table = build_rewrite_table();
    // Build name lookup: SymbolId → name string, for matching callees
    let symbol_names = symbols.all_names();
    let mut binding_types: HashMap<Binding, TyId> = HashMap::new();
    let mut hir_types: HashMap<HirId, TyId> = HashMap::new();
    let mut binding_min_length: HashMap<Binding, usize> = HashMap::new();

    // Collect parameter info for lambdas: which bindings are params of which lambda
    let mut lambda_params: HashMap<Binding, Vec<Binding>> = HashMap::new();
    let mut lambda_body_type: HashMap<Binding, TyId> = HashMap::new();
    collect_lambda_info(hir, arena, &mut lambda_params);

    for _ in 0..MAX_ITERS {
        let mut changed = false;

        // Forward type inference
        changed |= infer_types(
            hir,
            &interner,
            arena,
            &mut binding_types,
            &mut hir_types,
            &lambda_params,
            &mut lambda_body_type,
            &symbol_names,
            &mut binding_min_length,
        );

        // Rewrite stdlib calls to intrinsics where types prove it's safe
        changed |= rewrite_calls(
            hir,
            &interner,
            arena,
            &rewrite_table,
            &symbol_names,
            &binding_types,
            &hir_types,
        );

        if !changed {
            break;
        }
    }

    // Signal narrowing: strip SIG_ERROR from calls with provably typed args
    super::narrow::narrow_signals(
        hir,
        &interner,
        arena,
        &symbol_names,
        &hir_types,
        &binding_min_length,
    );

    // Signal re-propagation: recompute parent signals bottom-up
    super::narrow::repropagate_signals(hir);

    TypeInfo { hir_types }
}

/// Collect which bindings are lambda definitions and what their params are.
fn collect_lambda_info(
    hir: &Hir,
    _arena: &BindingArena,
    lambda_params: &mut HashMap<Binding, Vec<Binding>>,
) {
    match &hir.kind {
        HirKind::Letrec { bindings, body } | HirKind::Let { bindings, body } => {
            for (binding, value) in bindings {
                if let HirKind::Lambda { params, .. } = &value.kind {
                    lambda_params.insert(*binding, params.clone());
                }
                collect_lambda_info(value, _arena, lambda_params);
            }
            collect_lambda_info(body, _arena, lambda_params);
        }
        _ => {
            hir.for_each_child(|child| collect_lambda_info(child, _arena, lambda_params));
        }
    }
}

/// Forward type inference pass. Returns true if any types changed.
#[allow(clippy::too_many_arguments)]
fn infer_types(
    hir: &Hir,
    interner: &TypeInterner,
    arena: &BindingArena,
    binding_types: &mut HashMap<Binding, TyId>,
    hir_types: &mut HashMap<HirId, TyId>,
    lambda_params: &HashMap<Binding, Vec<Binding>>,
    lambda_body_type: &mut HashMap<Binding, TyId>,
    symbol_names: &HashMap<u32, String>,
    binding_min_length: &mut HashMap<Binding, usize>,
) -> bool {
    let ty = infer_node(
        hir,
        interner,
        arena,
        binding_types,
        hir_types,
        lambda_params,
        lambda_body_type,
        symbol_names,
        binding_min_length,
    );
    let old = hir_types.insert(hir.id, ty);
    old != Some(ty)
}

/// Infer the type of a single HIR node.
#[allow(clippy::too_many_arguments)]
fn infer_node(
    hir: &Hir,
    interner: &TypeInterner,
    arena: &BindingArena,
    binding_types: &mut HashMap<Binding, TyId>,
    hir_types: &mut HashMap<HirId, TyId>,
    lambda_params: &HashMap<Binding, Vec<Binding>>,
    lambda_body_type: &mut HashMap<Binding, TyId>,
    symbol_names: &HashMap<u32, String>,
    binding_min_length: &mut HashMap<Binding, usize>,
) -> TyId {
    macro_rules! recurse {
        ($e:expr) => {
            infer_node(
                $e,
                interner,
                arena,
                binding_types,
                hir_types,
                lambda_params,
                lambda_body_type,
                symbol_names,
                binding_min_length,
            )
        };
    }

    match &hir.kind {
        // Literals
        HirKind::Nil => TypeInterner::NIL,
        HirKind::Bool(_) => TypeInterner::BOOL,
        HirKind::Int(_) => TypeInterner::INT,
        HirKind::Float(_) => TypeInterner::FLOAT,
        HirKind::String(_) => TypeInterner::STRING,
        HirKind::Keyword(_) => TypeInterner::KEYWORD,
        HirKind::EmptyList => TypeInterner::EMPTY_LIST,

        // Variable reference
        HirKind::Var(binding) => binding_types
            .get(binding)
            .copied()
            .unwrap_or(TypeInterner::TOP),

        // Intrinsic operations — known return types
        HirKind::Intrinsic { op, args } => {
            for arg in args {
                let ty = recurse!(arg);
                hir_types.insert(arg.id, ty);
            }
            intrinsic_return_type(*op, args, interner, hir_types)
        }

        // Let/Letrec — seed binding types from init values
        HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
            for (binding, init) in bindings {
                let ty = recurse!(init);
                hir_types.insert(init.id, ty);
                // For lambda bindings, track their body's return type
                if let HirKind::Lambda { body: lam_body, .. } = &init.kind {
                    let body_ty = hir_types
                        .get(&lam_body.id)
                        .copied()
                        .unwrap_or(TypeInterner::TOP);
                    let old = lambda_body_type
                        .get(binding)
                        .copied()
                        .unwrap_or(TypeInterner::BOTTOM);
                    let joined = interner.join(old, body_ty);
                    lambda_body_type.insert(*binding, joined);
                } else {
                    let old = binding_types
                        .get(binding)
                        .copied()
                        .unwrap_or(TypeInterner::BOTTOM);
                    let joined = interner.join(old, ty);
                    binding_types.insert(*binding, joined);
                    // Track min_length for array constructor bindings
                    if ty == TypeInterner::MUTABLE_ARRAY || ty == TypeInterner::ARRAY {
                        if let Some(len) = unwrap_to_call(init) {
                            binding_min_length.insert(*binding, len);
                        }
                    }
                }
            }
            let body_ty = recurse!(body);
            hir_types.insert(body.id, body_ty);
            body_ty
        }

        // Lambda — infer body type and track return type
        HirKind::Lambda { body, .. } => {
            let body_ty = recurse!(body);
            hir_types.insert(body.id, body_ty);
            // We return Top for the lambda value itself — it's a closure
            TypeInterner::TOP
        }

        // If — join branches, with type guard narrowing
        HirKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            let _cond_ty = recurse!(cond);
            hir_types.insert(cond.id, _cond_ty);

            // Type guard narrowing: if cond is a type predicate call,
            // narrow the binding's type in the true branch
            let guard = extract_type_guard(cond, arena);
            let saved_types: Vec<(Binding, Option<TyId>)>;
            if let Some((binding, narrow_ty)) = guard {
                saved_types = vec![(binding, binding_types.get(&binding).copied())];
                let old = binding_types
                    .get(&binding)
                    .copied()
                    .unwrap_or(TypeInterner::TOP);
                let narrowed = interner.meet(old, narrow_ty);
                binding_types.insert(binding, narrowed);
            } else {
                saved_types = Vec::new();
            }

            let then_ty = recurse!(then_branch);
            hir_types.insert(then_branch.id, then_ty);

            // Restore type environment for else branch
            for (binding, saved) in &saved_types {
                match saved {
                    Some(ty) => {
                        binding_types.insert(*binding, *ty);
                    }
                    None => {
                        binding_types.remove(binding);
                    }
                }
            }

            let else_ty = recurse!(else_branch);
            hir_types.insert(else_branch.id, else_ty);

            interner.join(then_ty, else_ty)
        }

        // Call — forward arg types to callee params; result = callee return type
        HirKind::Call { func, args, .. } => {
            let _func_ty = recurse!(func);
            hir_types.insert(func.id, _func_ty);

            let arg_types: Vec<TyId> = args
                .iter()
                .map(|a| {
                    let ty = recurse!(&a.expr);
                    hir_types.insert(a.expr.id, ty);
                    ty
                })
                .collect();

            // Forward arg types to callee params.
            // Handle both Var(b) and DerefCell { Var(b) } (letrec recursive calls).
            let callee_binding = unwrap_callee_binding(func);
            if let Some(callee_binding) = callee_binding {
                if let Some(params) = lambda_params.get(&callee_binding) {
                    for (i, param) in params.iter().enumerate() {
                        if let Some(&arg_ty) = arg_types.get(i) {
                            // Don't forward Top — it poisons the parameter type
                            // and prevents convergence in recursive functions.
                            if arg_ty != TypeInterner::TOP {
                                let old = binding_types
                                    .get(param)
                                    .copied()
                                    .unwrap_or(TypeInterner::BOTTOM);
                                let joined = interner.join(old, arg_ty);
                                binding_types.insert(*param, joined);
                            }
                        }
                    }
                }
                // Return type = whatever the callee's body returns.
                // Only use BOTTOM for known lambdas (in lambda_params) where the
                // body type hasn't been computed yet. For unknown callees (primitives,
                // imports), return TOP to avoid unsound rewrites.
                if lambda_params.contains_key(&callee_binding) {
                    let ret_ty = lambda_body_type
                        .get(&callee_binding)
                        .copied()
                        .unwrap_or(TypeInterner::BOTTOM);
                    return ret_ty;
                }

                // Primitive return type inference for unresolved callees
                let callee_sym = arena.get(callee_binding).name;
                if let Some(name) = symbol_names.get(&callee_sym.0) {
                    let prim_ty = primitive_return_type(name, &arg_types, interner);
                    if prim_ty != TypeInterner::TOP {
                        return prim_ty;
                    }
                }
            }

            TypeInterner::TOP
        }

        // Begin/Block — type is last expression
        HirKind::Begin(exprs) => {
            let mut ty = TypeInterner::NIL;
            for expr in exprs {
                ty = recurse!(expr);
                hir_types.insert(expr.id, ty);
            }
            ty
        }
        HirKind::Block { body, .. } => {
            let mut ty = TypeInterner::NIL;
            for expr in body {
                ty = recurse!(expr);
                hir_types.insert(expr.id, ty);
            }
            ty
        }

        // And/Or — conservative: Top
        HirKind::And(_) | HirKind::Or(_) => {
            hir.for_each_child(|child| {
                let ty = recurse!(child);
                hir_types.insert(child.id, ty);
            });
            TypeInterner::TOP
        }

        // Loop — recurse into body
        HirKind::Loop { bindings, body } => {
            for (binding, init) in bindings {
                let ty = recurse!(init);
                hir_types.insert(init.id, ty);
                let old = binding_types
                    .get(binding)
                    .copied()
                    .unwrap_or(TypeInterner::BOTTOM);
                binding_types.insert(*binding, interner.join(old, ty));
            }
            let body_ty = recurse!(body);
            hir_types.insert(body.id, body_ty);
            body_ty
        }

        // Assign/Define — update binding type
        HirKind::Assign { target, value }
        | HirKind::Define {
            binding: target,
            value,
        } => {
            let ty = recurse!(value);
            hir_types.insert(value.id, ty);
            let old = binding_types
                .get(target)
                .copied()
                .unwrap_or(TypeInterner::BOTTOM);
            binding_types.insert(*target, interner.join(old, ty));
            // Track min_length for array constructor bindings
            if ty == TypeInterner::MUTABLE_ARRAY || ty == TypeInterner::ARRAY {
                if let Some(call) = unwrap_to_call(value) {
                    binding_min_length.insert(*target, call);
                }
            }
            ty
        }

        // MakeCell — propagate inner value type
        HirKind::MakeCell { value } => {
            let ty = recurse!(value);
            hir_types.insert(value.id, ty);
            ty
        }

        // DerefCell — return binding type if cell is Var(b)
        HirKind::DerefCell { cell } => {
            let ty = recurse!(cell);
            hir_types.insert(cell.id, ty);
            if let HirKind::Var(b) = &cell.kind {
                binding_types.get(b).copied().unwrap_or(TypeInterner::TOP)
            } else {
                TypeInterner::TOP
            }
        }

        // SetCell — widen binding type
        HirKind::SetCell { cell, value } => {
            let cell_ty = recurse!(cell);
            hir_types.insert(cell.id, cell_ty);
            let val_ty = recurse!(value);
            hir_types.insert(value.id, val_ty);
            // Widen the binding's type with the new value
            if let HirKind::Var(b) = &cell.kind {
                let old = binding_types
                    .get(b)
                    .copied()
                    .unwrap_or(TypeInterner::BOTTOM);
                binding_types.insert(*b, interner.join(old, val_ty));
            }
            val_ty
        }

        // Everything else — recurse and return Top
        _ => {
            hir.for_each_child(|child| {
                let ty = recurse!(child);
                hir_types.insert(child.id, ty);
            });
            TypeInterner::TOP
        }
    }
}

/// Extract the binding from a callee expression.
/// Handles both `Var(b)` and `DerefCell { Var(b) }` (letrec recursive calls).
pub(super) fn unwrap_callee_binding(func: &Hir) -> Option<Binding> {
    match &func.kind {
        HirKind::Var(b) => Some(*b),
        HirKind::DerefCell { cell } => {
            if let HirKind::Var(b) = &cell.kind {
                Some(*b)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Extract arg count from a Call expression, unwrapping MakeCell if needed.
/// Returns Some(arg_count) for array/struct constructor calls.
fn unwrap_to_call(hir: &Hir) -> Option<usize> {
    match &hir.kind {
        HirKind::Call { args, .. } => Some(args.len()),
        HirKind::MakeCell { value } => unwrap_to_call(value),
        _ => None,
    }
}

/// Known return types for primitive (stdlib) function calls.
fn primitive_return_type(name: &str, arg_types: &[TyId], interner: &TypeInterner) -> TyId {
    match name {
        "array" => TypeInterner::ARRAY,
        "@array" => TypeInterner::MUTABLE_ARRAY,
        "struct" => TypeInterner::STRUCT,
        "@struct" => TypeInterner::MUTABLE_STRUCT,
        "string" => TypeInterner::STRING,
        "push" => {
            // push returns arg0 type (MutableArray passthrough)
            arg_types.first().copied().unwrap_or(TypeInterner::TOP)
        }
        "put" => {
            // put returns arg0 type (passthrough)
            arg_types.first().copied().unwrap_or(TypeInterner::TOP)
        }
        "abs" | "floor" | "ceil" | "round" => TypeInterner::NUMBER,
        "length" => TypeInterner::INT,
        "type" => TypeInterner::KEYWORD,
        "has?" | "empty?" | "contains?" => TypeInterner::BOOL,
        "string?" | "int?" | "integer?" | "float?" | "number?" | "nil?" | "boolean?"
        | "keyword?" | "symbol?" | "pair?" | "list?" | "array?" | "struct?" | "bytes?"
        | "even?" | "odd?" | "closure?" | "fiber?" | "box?" | "ptr?" | "pointer?" => {
            TypeInterner::BOOL
        }
        "string/contains?"
        | "string-contains?"
        | "string/starts-with?"
        | "string-starts-with?"
        | "string/ends-with?"
        | "string-ends-with?" => TypeInterner::BOOL,
        "number->string" => TypeInterner::STRING,
        _ => {
            let _ = (arg_types, interner);
            TypeInterner::TOP
        }
    }
}

/// Known return types for intrinsic operations.
fn intrinsic_return_type(
    op: IntrinsicOp,
    args: &[Hir],
    interner: &TypeInterner,
    hir_types: &HashMap<HirId, TyId>,
) -> TyId {
    match op {
        // Arithmetic: returns the join of arg types within Number
        IntrinsicOp::Add | IntrinsicOp::Sub | IntrinsicOp::Mul | IntrinsicOp::Div => {
            let mut ty = TypeInterner::BOTTOM;
            for arg in args {
                let arg_ty = hir_types.get(&arg.id).copied().unwrap_or(TypeInterner::TOP);
                ty = interner.join(ty, arg_ty);
            }
            // Clamp to Number (intrinsics only operate on numbers)
            if interner.subtype(ty, TypeInterner::NUMBER) {
                ty
            } else {
                TypeInterner::NUMBER
            }
        }
        IntrinsicOp::Rem => TypeInterner::NUMBER,
        IntrinsicOp::Mod => TypeInterner::INT,

        // Comparison: returns Bool
        IntrinsicOp::Eq
        | IntrinsicOp::Ne
        | IntrinsicOp::Lt
        | IntrinsicOp::Gt
        | IntrinsicOp::Le
        | IntrinsicOp::Ge => TypeInterner::BOOL,

        // Logical: returns Bool
        IntrinsicOp::Not => TypeInterner::BOOL,

        // Type predicates: return Bool
        IntrinsicOp::IsNil
        | IntrinsicOp::IsEmpty
        | IntrinsicOp::IsBool
        | IntrinsicOp::IsInt
        | IntrinsicOp::IsFloat
        | IntrinsicOp::IsString
        | IntrinsicOp::IsKeyword
        | IntrinsicOp::IsSymbol
        | IntrinsicOp::IsPair
        | IntrinsicOp::IsArray
        | IntrinsicOp::IsStruct
        | IntrinsicOp::IsSet
        | IntrinsicOp::IsBytes
        | IntrinsicOp::IsBox
        | IntrinsicOp::IsClosure
        | IntrinsicOp::IsFiber
        | IntrinsicOp::Identical => TypeInterner::BOOL,

        // Conversions
        IntrinsicOp::Int => TypeInterner::INT,
        IntrinsicOp::Float => TypeInterner::FLOAT,

        // Pair
        IntrinsicOp::Pair => TypeInterner::TOP,
        IntrinsicOp::First | IntrinsicOp::Rest => TypeInterner::TOP,

        // Bitwise: return Int
        IntrinsicOp::BitAnd
        | IntrinsicOp::BitOr
        | IntrinsicOp::BitXor
        | IntrinsicOp::BitNot
        | IntrinsicOp::Shl
        | IntrinsicOp::Shr => TypeInterner::INT,

        // TypeOf returns keyword
        IntrinsicOp::TypeOf => TypeInterner::KEYWORD,

        // Length returns Int
        IntrinsicOp::Length => TypeInterner::INT,

        // Everything else
        _ => TypeInterner::TOP,
    }
}

/// Extract type guard information from an If condition.
/// Returns `(binding, narrowed_type)` if the condition is a type predicate.
fn extract_type_guard(cond: &Hir, _arena: &BindingArena) -> Option<(Binding, TyId)> {
    match &cond.kind {
        // Direct type predicate: (%int? x), (%float? x), etc.
        HirKind::Intrinsic { op, args } if args.len() == 1 => {
            let binding = match &args[0].kind {
                HirKind::Var(b) => *b,
                HirKind::DerefCell { cell } => {
                    if let HirKind::Var(b) = &cell.kind {
                        *b
                    } else {
                        return None;
                    }
                }
                _ => return None,
            };
            let ty = match op {
                IntrinsicOp::IsInt => TypeInterner::INT,
                IntrinsicOp::IsFloat => TypeInterner::FLOAT,
                IntrinsicOp::IsString => TypeInterner::STRING,
                IntrinsicOp::IsKeyword => TypeInterner::KEYWORD,
                IntrinsicOp::IsSymbol => TypeInterner::SYMBOL,
                IntrinsicOp::IsBool => TypeInterner::BOOL,
                IntrinsicOp::IsNil => TypeInterner::NIL,
                _ => return None,
            };
            Some((binding, ty))
        }
        // Call to type predicate: (number? x), (integer? x), etc.
        // These haven't been rewritten to intrinsics yet since they're stdlib calls
        // Stdlib type predicate calls are handled after they get rewritten to intrinsics
        HirKind::Call { .. } => None,
        _ => None,
    }
}

/// Rewrite stdlib calls to intrinsics where types prove it's safe.
/// Returns true if any rewrites were applied.
fn rewrite_calls(
    hir: &mut Hir,
    interner: &TypeInterner,
    arena: &BindingArena,
    rewrite_table: &HashMap<&str, RewriteRule>,
    symbol_names: &HashMap<u32, String>,
    binding_types: &HashMap<Binding, TyId>,
    hir_types: &HashMap<HirId, TyId>,
) -> bool {
    let mut changed = false;

    // First, try to rewrite this node
    if let HirKind::Call { func, args, .. } = &hir.kind {
        if let HirKind::Var(callee_binding) = &func.kind {
            let callee_sym = arena.get(*callee_binding).name;
            // Look up name from SymbolId
            if let Some(name) = symbol_names.get(&callee_sym.0) {
                if let Some(rule) = rewrite_table.get(name.as_str()) {
                    let arg_count = args.len();
                    if arg_count >= rule.arity.0 && arg_count <= rule.arity.1 {
                        // Check type constraint
                        let types_ok = match rule.constraint {
                            None => true,
                            Some(constraint_ty) => args.iter().all(|arg| {
                                let arg_ty = hir_types
                                    .get(&arg.expr.id)
                                    .copied()
                                    .unwrap_or(TypeInterner::TOP);
                                interner.subtype(arg_ty, constraint_ty)
                            }),
                        };

                        if types_ok {
                            // Extract args and replace Call with Intrinsic
                            let intrinsic_args: Vec<Hir> =
                                if let HirKind::Call { args, .. } = &hir.kind {
                                    args.iter().map(|a| a.expr.clone()).collect()
                                } else {
                                    unreachable!()
                                };
                            let op = rule.op;
                            hir.kind = HirKind::Intrinsic {
                                op,
                                args: intrinsic_args,
                            };
                            hir.signal = Signal::silent();
                            changed = true;
                        }
                    }
                }
            }
        }
    }

    // Recurse into children (must use mutable access)
    changed |= rewrite_children(
        hir,
        interner,
        arena,
        rewrite_table,
        symbol_names,
        binding_types,
        hir_types,
    );

    changed
}

/// Recursively rewrite children of a HIR node.
fn rewrite_children(
    hir: &mut Hir,
    interner: &TypeInterner,
    arena: &BindingArena,
    rewrite_table: &HashMap<&str, RewriteRule>,
    symbol_names: &HashMap<u32, String>,
    binding_types: &HashMap<Binding, TyId>,
    hir_types: &HashMap<HirId, TyId>,
) -> bool {
    let mut changed = false;
    hir.for_each_child_mut(|child| {
        changed |= rewrite_calls(
            child,
            interner,
            arena,
            rewrite_table,
            symbol_names,
            binding_types,
            hir_types,
        );
    });
    changed
}
