// audited: 2026-10-06
//! The syntax-object accessors, the `squelch` and `attune` boundaries, `meta/origin`, and the SPIR-V primitives git, fn/git? and disgit.
//!
//! docs/macros.md
//! docs/impl/spirv.md
//! docs/signals/inference.md

use super::*;

pub(crate) fn prim_syntax_to_list(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let stx = match require_syntax(args, "syntax->list", ctx) {
        Ok(s) => s,
        Err(e) => return e,
    };
    match &stx.kind {
        SyntaxKind::List(items) => {
            let elems: Vec<Value> = items.iter().map(|item| ctx.syntax(*item)).collect();
            (SIG_OK, ctx.array(elems))
        }
        _ => (
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!(
                    "syntax->list: expected syntax list, got {}",
                    stx.kind_label()
                ),
            ),
        ),
    }
}

pub(crate) fn prim_syntax_first(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let stx = match require_syntax(args, "syntax-first", ctx) {
        Ok(s) => s,
        Err(e) => return e,
    };
    match &stx.kind {
        SyntaxKind::List(items) if !items.is_empty() => (SIG_OK, ctx.syntax(items[0])),
        SyntaxKind::List(_) => (
            SIG_ERROR,
            ctx.error("type-error", "syntax-first: expected non-empty syntax list"),
        ),
        _ => (
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!(
                    "syntax-first: expected syntax list, got {}",
                    stx.kind_label()
                ),
            ),
        ),
    }
}

pub(crate) fn prim_syntax_rest(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let stx = match require_syntax(args, "syntax-rest", ctx) {
        Ok(s) => s,
        Err(e) => return e,
    };
    match &stx.kind {
        SyntaxKind::List(items) if !items.is_empty() => {
            let rest = Syntax::list(&ctx.syntax_arena(), &items[1..], stx.span);
            (SIG_OK, ctx.syntax(rest))
        }
        SyntaxKind::List(_) => (
            SIG_ERROR,
            ctx.error("type-error", "syntax-rest: expected non-empty syntax list"),
        ),
        _ => (
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!(
                    "syntax-rest: expected syntax list, got {}",
                    stx.kind_label()
                ),
            ),
        ),
    }
}

pub(crate) fn prim_syntax_e(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let stx = match require_syntax(args, "syntax-e", ctx) {
        Ok(s) => s,
        Err(e) => return e,
    };
    match &stx.kind {
        SyntaxKind::Nil => (SIG_OK, Value::NIL),
        SyntaxKind::Bool(b) => (SIG_OK, Value::bool(*b)),
        SyntaxKind::Int(n) => (SIG_OK, Value::int(*n)),
        SyntaxKind::Float(f) => (SIG_OK, Value::float(*f)),
        SyntaxKind::String(s) => (SIG_OK, ctx.string(*s)),
        SyntaxKind::Keyword(k) => (SIG_OK, Value::keyword(k)),
        SyntaxKind::Symbol(name) => {
            // Intern into this instance's table via the driving VM. Mirrors the
            // pattern in prim_gensym.
            let symbols_ptr = ctx.vm().symbols_ptr;
            if symbols_ptr.is_null() {
                return (
                    SIG_ERROR,
                    ctx.error("internal-error", "syntax-e: symbol table not available"),
                );
            }
            let id = unsafe { (*symbols_ptr).intern(name) };
            (SIG_OK, Value::symbol(id))
        }
        // Compounds: return the syntax object unchanged.
        _ => (SIG_OK, args[0]),
    }
}

/// `(squelch spec f)`: a new closure over `f`'s code and environment whose
/// boundary turns the signals in `spec` into a `:signal-violation` error.
///
/// `spec` comes first, as it does for `attune`, and resolves through
/// `resolve_signal_bits`: a keyword, set, array, list, or integer. The masks of
/// nested squelches join, so a squelch of a squelched closure blocks both. The
/// closure is checked first, so a call with its arguments swapped names it.
pub(crate) fn prim_squelch(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let closure = match closure_argument(&args[1], "squelch", ctx) {
        Ok(c) => c,
        Err(err) => return err,
    };
    let blocked = match crate::primitives::fibers::resolve_signal_bits(&args[0], "squelch", ctx) {
        Ok(bits) => bits,
        Err(err) => return err,
    };
    let wrapped = Closure::new(
        closure.template,
        closure.env,
        closure.squelch_mask.union(blocked),
    );
    (SIG_OK, ctx.closure(wrapped))
}

/// `(attune spec f)`: a new closure over `f`'s code and environment whose
/// boundary lets through only the signals in `spec`, and turns every other
/// signal a program can raise into a `:signal-violation` error. It is
/// `(squelch (complement spec) f)`.
pub(crate) fn prim_attune(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let closure = match closure_argument(&args[1], "attune", ctx) {
        Ok(c) => c,
        Err(err) => return err,
    };
    let permitted = match crate::primitives::fibers::resolve_signal_bits(&args[0], "attune", ctx) {
        Ok(bits) => bits,
        Err(err) => return err,
    };
    let wrapped = Closure::new(
        closure.template,
        closure.env,
        closure
            .squelch_mask
            .union(crate::signals::CAP_MASK.subtract(permitted)),
    );
    (SIG_OK, ctx.closure(wrapped))
}

/// The closure `squelch` or `attune`, named `prim`, takes second, or the
/// type error for any other value.
fn closure_argument<'v>(
    value: &'v Value,
    prim: &str,
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
) -> Result<&'v Closure, (SignalBits, Value)> {
    value.as_closure().ok_or_else(|| {
        (
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!(
                    "{}: second argument must be a closure, got {}",
                    prim,
                    value.type_name()
                ),
            ),
        )
    })
}

/// Return the source location of a closure as `{:file :line :col}`, or `nil`.
///
/// `(meta/origin f)` reads the defining span the closure's code payload
/// carries. Returns `nil` if `f` is not a closure, the closure has no origin,
/// or the span names no file.
pub(crate) fn prim_meta_origin(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let val = args[0];
    let closure_rc = match val.as_closure() {
        Some(c) => c,
        None => return (SIG_OK, Value::NIL),
    };
    let origin = match closure_rc.template.origin() {
        Some(span) => span,
        None => return (SIG_OK, Value::NIL),
    };
    let file = match origin.file() {
        Some(f) => f,
        None => return (SIG_OK, Value::NIL),
    };
    let mut fields = std::collections::BTreeMap::new();
    fields.insert(TableKey::keyword("file"), ctx.string(file));
    fields.insert(TableKey::keyword("line"), Value::int(origin.line as i64));
    fields.insert(TableKey::keyword("col"), Value::int(origin.col as i64));
    (SIG_OK, ctx.struct_from(fields))
}

/// Eagerly compile SPIR-V, cache it on the VM, return the closure.
///
/// `(git f [workgroup-size])` compiles the closure to SPIR-V and caches the
/// bytes in the VM's SPIR-V cache under `f`'s bytecode and the workgroup size,
/// 256 when none is given. Returns `f`; every closure over the same payload
/// sees the cached SPIR-V.
pub(crate) fn prim_git(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    #[cfg(not(feature = "mlir"))]
    {
        let _ = args;
        (
            SIG_ERROR,
            ctx.error("mlir-error", "git: requires --features mlir"),
        )
    }
    #[cfg(feature = "mlir")]
    {
        let size = match workgroup_arg(ctx, args, "git") {
            Ok(size) => size,
            Err(raised) => return raised,
        };
        let closure = prim_arg!(ctx, args, 0, as_closure, "git", "closure");
        // Fast path: already cached
        if ctx.vm().spirv_for(&closure.template, size).is_some() {
            return (SIG_OK, args[0]);
        }
        // Check GPU eligibility upfront
        if !closure.template.is_gpu_candidate() {
            return (
                SIG_ERROR,
                ctx.error("mlir-error", "git: closure is not GPU-eligible"),
            );
        }
        if !closure.template.has_lir() {
            return (
                SIG_ERROR,
                ctx.error("mlir-error", "git: closure has no LIR"),
            );
        }
        // Delegate to VM via SIG_QUERY for MlirCache access.
        (SIG_QUERY, {
            let inner = ctx.pair(args[0], Value::int(size.get() as i64));
            ctx.pair(Value::keyword("git"), inner)
        })
    }
}

/// The workgroup size a SPIR-V primitive's optional second argument names, or
/// 256 when it is absent. A bad size raises as `who`'s own error, so a
/// primitive validates it once, at its boundary (docs/impl/spirv.md).
pub(crate) fn workgroup_arg(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
    who: &str,
) -> Result<crate::vm::core::WorkgroupSize, (SignalBits, Value)> {
    match args.get(1) {
        None => Ok(crate::vm::core::WorkgroupSize::DEFAULT),
        Some(&v) => crate::vm::core::WorkgroupSize::of_value(v, who)
            .map_err(|(kind, msg)| (SIG_ERROR, ctx.error(kind, msg))),
    }
}

/// `(fn/git? f [workgroup-size])` — true if the VM caches SPIR-V for the
/// closure at that size, 256 when none is given.
pub(crate) fn prim_fn_git(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let size = match workgroup_arg(ctx, args, "fn/git?") {
        Ok(size) => size,
        Err(raised) => return raised,
    };
    let cached = args[0]
        .as_closure()
        .is_some_and(|closure| ctx.vm().spirv_for(&closure.template, size).is_some());
    (SIG_OK, Value::bool(cached))
}

/// `(disgit f [workgroup-size])` — the SPIR-V bytes the VM caches for the
/// closure at that size, 256 when none is given.
///
/// Errors if `f` is not a closure or nothing is cached at that size.
pub(crate) fn prim_disgit(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let size = match workgroup_arg(ctx, args, "disgit") {
        Ok(size) => size,
        Err(raised) => return raised,
    };
    let closure = prim_arg!(ctx, args, 0, as_closure, "disgit", "closure");
    let cached = ctx
        .vm()
        .spirv_for(&closure.template, size)
        .map(<[u8]>::to_vec);
    match cached {
        Some(bytes) => (SIG_OK, ctx.bytes(bytes)),
        None => (
            SIG_ERROR,
            ctx.error(
                "mlir-error",
                format!(
                    "disgit: no SPIR-V is cached for the closure at workgroup size {}",
                    size.get()
                ),
            ),
        ),
    }
}
