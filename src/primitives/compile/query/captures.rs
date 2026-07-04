use super::*;
use crate::primitives::ctx::NativeCtx;

/// (compile/captures analysis :fn-name) → [{:name "x" :kind :value :mutated false}]
pub(crate) fn prim_compile_captures(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/captures", ctx) {
        Ok(h) => h,
        Err(e) => return e,
    };
    let name = match resolve_name(args, 1, "compile/captures", ctx) {
        Ok(n) => n,
        Err(e) => return e,
    };

    let symbols_ptr = ctx.vm().symbols_ptr;
    if symbols_ptr.is_null() {
        return (
            SIG_ERROR,
            ctx.error("runtime-error", "compile/captures: no symbol table"),
        );
    }
    let symbols = unsafe { &*symbols_ptr };

    // Find the Lambda for this function name.
    match find_lambda_captures(&handle.hir, &handle.arena, symbols, &name, ctx) {
        Some(captures) => (SIG_OK, ctx.array(captures)),
        None => (
            SIG_ERROR,
            ctx.error(
                "lookup-error",
                format!("compile/captures: no function '{}' in analysis", name),
            ),
        ),
    }
}

pub(crate) fn find_lambda_captures(
    hir: &Hir,
    arena: &BindingArena,
    symbols: &crate::symbol::SymbolTable,
    target: &str,
    ctx: &mut NativeCtx,
) -> Option<Vec<Value>> {
    // The `.or_else(|| recurse(..))` chains of the region-threaded form are
    // rewritten as sequential `if let Some` returns: each recursive call takes
    // `&mut ctx` and must drop that borrow before the next call takes it, which
    // a closure chain (two live `&mut ctx` captures) would not allow.
    match &hir.kind {
        HirKind::Letrec { bindings, body } | HirKind::Let { bindings, body } => {
            for (binding, value) in bindings {
                if let Some(name) = symbols.name(arena.get(*binding).name) {
                    if name == target {
                        if let HirKind::Lambda { captures, .. } = &value.kind {
                            return Some(captures_to_values(captures, arena, symbols, ctx));
                        }
                    }
                }
                if let Some(result) = find_lambda_captures(value, arena, symbols, target, ctx) {
                    return Some(result);
                }
            }
            find_lambda_captures(body, arena, symbols, target, ctx)
        }
        HirKind::Define { binding, value } => {
            if let Some(name) = symbols.name(arena.get(*binding).name) {
                if name == target {
                    if let HirKind::Lambda { captures, .. } = &value.kind {
                        return Some(captures_to_values(captures, arena, symbols, ctx));
                    }
                }
            }
            find_lambda_captures(value, arena, symbols, target, ctx)
        }
        HirKind::Lambda { body, .. } => find_lambda_captures(body, arena, symbols, target, ctx),
        HirKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            if let Some(r) = find_lambda_captures(cond, arena, symbols, target, ctx) {
                return Some(r);
            }
            if let Some(r) = find_lambda_captures(then_branch, arena, symbols, target, ctx) {
                return Some(r);
            }
            find_lambda_captures(else_branch, arena, symbols, target, ctx)
        }
        HirKind::Begin(exprs) | HirKind::And(exprs) | HirKind::Or(exprs) => {
            for e in exprs {
                if let Some(r) = find_lambda_captures(e, arena, symbols, target, ctx) {
                    return Some(r);
                }
            }
            None
        }
        HirKind::Block { body, .. } => {
            for e in body {
                if let Some(r) = find_lambda_captures(e, arena, symbols, target, ctx) {
                    return Some(r);
                }
            }
            None
        }
        HirKind::Call { func, args, .. } => {
            if let Some(r) = find_lambda_captures(func, arena, symbols, target, ctx) {
                return Some(r);
            }
            for arg in args {
                if let Some(r) = find_lambda_captures(&arg.expr, arena, symbols, target, ctx) {
                    return Some(r);
                }
            }
            None
        }
        HirKind::Assign { value, .. } => find_lambda_captures(value, arena, symbols, target, ctx),
        HirKind::While { cond, body } => {
            if let Some(r) = find_lambda_captures(cond, arena, symbols, target, ctx) {
                return Some(r);
            }
            find_lambda_captures(body, arena, symbols, target, ctx)
        }
        HirKind::Match { value, arms } => {
            if let Some(r) = find_lambda_captures(value, arena, symbols, target, ctx) {
                return Some(r);
            }
            for (_, guard, body) in arms {
                if let Some(g) = guard {
                    if let Some(r) = find_lambda_captures(g, arena, symbols, target, ctx) {
                        return Some(r);
                    }
                }
                if let Some(r) = find_lambda_captures(body, arena, symbols, target, ctx) {
                    return Some(r);
                }
            }
            None
        }
        HirKind::Emit { value: e, .. } | HirKind::Break { value: e, .. } => {
            find_lambda_captures(e, arena, symbols, target, ctx)
        }
        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            for (c, b) in clauses {
                if let Some(r) = find_lambda_captures(c, arena, symbols, target, ctx) {
                    return Some(r);
                }
                if let Some(r) = find_lambda_captures(b, arena, symbols, target, ctx) {
                    return Some(r);
                }
            }
            match else_branch.as_ref() {
                Some(e) => find_lambda_captures(e, arena, symbols, target, ctx),
                None => None,
            }
        }
        HirKind::Destructure { value, .. } => {
            find_lambda_captures(value, arena, symbols, target, ctx)
        }
        HirKind::Eval { expr, env } => {
            if let Some(r) = find_lambda_captures(expr, arena, symbols, target, ctx) {
                return Some(r);
            }
            find_lambda_captures(env, arena, symbols, target, ctx)
        }
        HirKind::Parameterize { bindings, body } => {
            for (p, v) in bindings {
                if let Some(r) = find_lambda_captures(p, arena, symbols, target, ctx) {
                    return Some(r);
                }
                if let Some(r) = find_lambda_captures(v, arena, symbols, target, ctx) {
                    return Some(r);
                }
            }
            find_lambda_captures(body, arena, symbols, target, ctx)
        }
        _ => None,
    }
}

pub(crate) fn captures_to_values(
    captures: &[crate::hir::CaptureInfo],
    arena: &BindingArena,
    symbols: &crate::symbol::SymbolTable,
    ctx: &mut NativeCtx,
) -> Vec<Value> {
    captures
        .iter()
        .map(|cap| {
            let inner = arena.get(cap.binding);
            let mut fields = BTreeMap::new();
            if let Some(name) = symbols.name(inner.name) {
                let name_val = ctx.string(name);
                fields.insert(kw("name"), name_val);
            }
            let kind = match cap.kind {
                // A `Recursive` self-edge is reported like a `Local` capture, keyed on the
                // binding's actual cell status: `lbox` when it keeps a cell (a sibling also
                // captures it), `value` when it is cell-free (captured only by itself).
                crate::hir::CaptureKind::Local | crate::hir::CaptureKind::Recursive { .. } => {
                    if inner.needs_capture() {
                        "lbox"
                    } else {
                        "value"
                    }
                }
                crate::hir::CaptureKind::Capture { .. } => "transitive",
            };
            fields.insert(kw("kind"), Value::keyword(kind));
            fields.insert(kw("mutated"), Value::bool(inner.is_mutated));
            ctx.struct_from(fields)
        })
        .collect()
}

/// (compile/captured-by analysis :name) → [{:name "make-handler" :line 20}]
/// Reverse lookup: which functions capture the named binding?
pub(crate) fn prim_compile_captured_by(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/captured-by", ctx) {
        Ok(h) => h,
        Err(e) => return e,
    };
    let name = match resolve_name(args, 1, "compile/captured-by", ctx) {
        Ok(n) => n,
        Err(e) => return e,
    };

    let symbols_ptr = ctx.vm().symbols_ptr;
    if symbols_ptr.is_null() {
        return (
            SIG_ERROR,
            ctx.error("runtime-error", "compile/captured-by: no symbol table"),
        );
    }
    let symbols = unsafe { &*symbols_ptr };

    // Find all lambdas whose captures include a binding named `name`.
    let mut results = Vec::new();
    find_capturers(
        &handle.hir,
        &handle.arena,
        symbols,
        &name,
        &mut results,
        ctx,
    );
    (SIG_OK, ctx.array(results))
}

pub(crate) fn find_capturers(
    hir: &Hir,
    arena: &BindingArena,
    symbols: &crate::symbol::SymbolTable,
    target: &str,
    results: &mut Vec<Value>,
    ctx: &mut NativeCtx,
) {
    match &hir.kind {
        HirKind::Letrec { bindings, body } | HirKind::Let { bindings, body } => {
            for (binding, value) in bindings {
                if let HirKind::Lambda { captures, .. } = &value.kind {
                    for cap in captures {
                        if let Some(cap_name) = symbols.name(arena.get(cap.binding).name) {
                            if cap_name == target {
                                let mut fields = BTreeMap::new();
                                if let Some(fn_name) = symbols.name(arena.get(*binding).name) {
                                    let name_val = ctx.string(fn_name);
                                    fields.insert(kw("name"), name_val);
                                }
                                fields.insert(kw("line"), Value::int(value.span.line as i64));
                                results.push(ctx.struct_from(fields));
                                break;
                            }
                        }
                    }
                }
                find_capturers(value, arena, symbols, target, results, ctx);
            }
            find_capturers(body, arena, symbols, target, results, ctx);
        }
        HirKind::Define { binding, value } => {
            if let HirKind::Lambda { captures, .. } = &value.kind {
                for cap in captures {
                    if let Some(cap_name) = symbols.name(arena.get(cap.binding).name) {
                        if cap_name == target {
                            let mut fields = BTreeMap::new();
                            if let Some(fn_name) = symbols.name(arena.get(*binding).name) {
                                let name_val = ctx.string(fn_name);
                                fields.insert(kw("name"), name_val);
                            }
                            fields.insert(kw("line"), Value::int(value.span.line as i64));
                            results.push(ctx.struct_from(fields));
                            break;
                        }
                    }
                }
            }
            find_capturers(value, arena, symbols, target, results, ctx);
        }
        HirKind::Lambda { body, .. } => find_capturers(body, arena, symbols, target, results, ctx),
        HirKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            find_capturers(cond, arena, symbols, target, results, ctx);
            find_capturers(then_branch, arena, symbols, target, results, ctx);
            find_capturers(else_branch, arena, symbols, target, results, ctx);
        }
        HirKind::Begin(exprs) | HirKind::And(exprs) | HirKind::Or(exprs) => {
            for e in exprs {
                find_capturers(e, arena, symbols, target, results, ctx);
            }
        }
        HirKind::Block { body, .. } => {
            for e in body {
                find_capturers(e, arena, symbols, target, results, ctx);
            }
        }
        HirKind::Call { func, args, .. } => {
            find_capturers(func, arena, symbols, target, results, ctx);
            for arg in args {
                find_capturers(&arg.expr, arena, symbols, target, results, ctx);
            }
        }
        HirKind::Assign { value, .. } => {
            find_capturers(value, arena, symbols, target, results, ctx)
        }
        HirKind::While { cond, body } => {
            find_capturers(cond, arena, symbols, target, results, ctx);
            find_capturers(body, arena, symbols, target, results, ctx);
        }
        HirKind::Match { value, arms } => {
            find_capturers(value, arena, symbols, target, results, ctx);
            for (_, guard, body) in arms {
                if let Some(g) = guard {
                    find_capturers(g, arena, symbols, target, results, ctx);
                }
                find_capturers(body, arena, symbols, target, results, ctx);
            }
        }
        HirKind::Emit { value: e, .. } | HirKind::Break { value: e, .. } => {
            find_capturers(e, arena, symbols, target, results, ctx);
        }
        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            for (c, b) in clauses {
                find_capturers(c, arena, symbols, target, results, ctx);
                find_capturers(b, arena, symbols, target, results, ctx);
            }
            if let Some(e) = else_branch {
                find_capturers(e, arena, symbols, target, results, ctx);
            }
        }
        HirKind::Destructure { value, .. } => {
            find_capturers(value, arena, symbols, target, results, ctx)
        }
        HirKind::Eval { expr, env } => {
            find_capturers(expr, arena, symbols, target, results, ctx);
            find_capturers(env, arena, symbols, target, results, ctx);
        }
        HirKind::Parameterize { bindings, body } => {
            for (p, v) in bindings {
                find_capturers(p, arena, symbols, target, results, ctx);
                find_capturers(v, arena, symbols, target, results, ctx);
            }
            find_capturers(body, arena, symbols, target, results, ctx);
        }
        _ => {}
    }
}
