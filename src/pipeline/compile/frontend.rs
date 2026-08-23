//! Shared file/syntax compilation front end: parse → epoch-migrate → macro
//! expand (with include splicing) → classify → analyze → regularize. Every
//! backend (bytecode, LIR, FHIR, the test-mode transforms) feeds off the HIR
//! this stage produces, so it lives apart from the per-backend lowering wrappers.

use super::*;

/// The tuple produced by the file/syntax front end, shared by the source and
/// syntax test-compile entry points (see `lower_test_frontend`).
///
/// Returns `(hir, arena, expander, prim_values, signal_projection)`.
/// Callers that don't need all fields can ignore the extras.
#[allow(clippy::type_complexity)]
pub(super) type Frontend = (
    crate::hir::Hir,
    BindingArena,
    crate::syntax::Expander,
    std::collections::HashMap<crate::hir::Binding, crate::value::Value>,
    Option<std::collections::HashMap<String, crate::signals::Signal>>,
);

pub(super) type FrontendResult = Result<Frontend, String>;

/// Shared front-end for file compilation: parse, epoch migration, macro
/// expansion (with file-scope stamping and include resolution), analysis,
/// error surfacing, tail-call marking, and functionalization.
pub(super) fn compile_file_frontend(
    source: &str,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
    source_name: &str,
) -> FrontendResult {
    compile_file_frontend_xform(source, symbols, cctx, source_name, |forms, _scope| forms)
}

/// Like `compile_file_frontend`, but applies `xform` to the macro-expanded
/// top-level forms before classification/analysis. The fault-barrier test
/// compilation mode (`compile_barrier_module`) uses this to wrap each test
/// form in a captured thunk while leaving `def`/`var` setup forms intact —
/// preserving the whole-module `analyze_file_letrec` path (binding resolution,
/// capture analysis, signal inference, epoch) that per-form `eval` cannot.
///
/// Reads + compiles inside a transient region so the per-compilation scratch
/// allocations are reclaimed once the frontend result is promoted out.
pub(super) fn compile_file_frontend_xform(
    source: &str,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
    source_name: &str,
    xform: impl FnOnce(Vec<Syntax>, crate::syntax::ScopeId) -> Vec<Syntax>,
) -> FrontendResult {
    with_transient(cctx.heap_ptr(), || {
        let syntaxes = read_syntax_all_for(source, source_name)?;
        compile_syntaxes_frontend_xform_inner(syntaxes, symbols, cctx, source_name, xform)
    })
}

/// Like `compile_file_frontend_xform`, but starts from already-read top-level
/// `syntaxes` instead of source text. Lets a caller parse once and compile the
/// resulting forms elsewhere — the test runner reads a legacy multi-form file in
/// the main VM and ships the forms to a worker, which compiles + runs them with
/// its OWN stdlib (so the file's runtime `import`s and the worker's `ev/run`
/// scheduler share one set of dynamic parameters). Epoch migration, macro
/// expansion, the `xform`, and analysis are identical to the text path.
pub(super) fn compile_syntaxes_frontend_xform(
    syntaxes: Vec<Syntax>,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
    source_name: &str,
    xform: impl FnOnce(Vec<Syntax>, crate::syntax::ScopeId) -> Vec<Syntax>,
) -> FrontendResult {
    with_transient(cctx.heap_ptr(), || {
        compile_syntaxes_frontend_xform_inner(syntaxes, symbols, cctx, source_name, xform)
    })
}

/// Shared frontend worker (epoch-migrate → expand → `xform` → classify →
/// analyze). Run inside `with_transient` by every public caller so
/// per-compilation scratch is reclaimed while the result is promoted out.
#[allow(clippy::type_complexity)]
fn compile_syntaxes_frontend_xform_inner(
    mut syntaxes: Vec<Syntax>,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
    source_name: &str,
    xform: impl FnOnce(Vec<Syntax>, crate::syntax::ScopeId) -> Vec<Syntax>,
) -> FrontendResult {
    intern_primitive_names(symbols);
    let profile = std::env::var("ELLE_PROFILE").is_ok();
    let ft0 = std::time::Instant::now();
    let fmark = |label: &str| {
        if profile {
            eprintln!(
                "[elle-profile] frontend {source_name} {label}: {:?}",
                ft0.elapsed()
            );
        }
    };

    let source_epoch = crate::epoch::extract_epoch(&mut syntaxes)?;
    if let Some(epoch) = source_epoch {
        crate::epoch::migrate_forms(&mut syntaxes, epoch)?;
    }
    fmark("parse+epoch");

    let (expanded_forms, mut expander, meta) =
        cctx.with_macro_expansion(|macro_vm, mut expander, meta| {
            let mut pending: std::collections::VecDeque<Syntax> = if source_name.starts_with('<') {
                syntaxes.into()
            } else {
                let file_scope = expander.fresh_scope();
                syntaxes
                    .into_iter()
                    .map(|s| expander.stamp_scope(s, file_scope))
                    .collect()
            };
            let mut expanded_forms = Vec::new();
            let mut included: HashSet<String> = HashSet::from([source_name.to_string()]);
            while let Some(syntax) = pending.pop_front() {
                if resolve_and_splice_include(&syntax, source_name, &mut pending, &mut included)? {
                    continue;
                }
                expanded_forms.push(expander.expand(syntax, symbols, macro_vm)?);
            }
            Ok::<_, String>((expanded_forms, expander, meta))
        })?;
    fmark("expand(include+macros)");

    // A fresh scope for any accumulator/temporaries `xform` injects, minted after
    // expansion so it cannot collide with a scope the expander already assigned.
    // Stamping the injected symbols with it makes them hygienic (invisible to
    // user references and to `(environment)`); the identity xform ignores it.
    let acc_scope = expander.fresh_scope();
    let expanded_forms = xform(expanded_forms, acc_scope);

    let forms: Vec<FileForm> = expanded_forms.iter().map(classify_form).collect();
    let span = if expanded_forms.is_empty() {
        Span::synthetic()
    } else {
        expanded_forms[0]
            .span
            .merge(&expanded_forms[expanded_forms.len() - 1].span)
    };

    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new_with_primitives(
        symbols,
        &mut arena,
        meta.signals.clone(),
        meta.arities.clone(),
    );
    analyzer.set_compile_ctx(cctx);
    let effective_epoch = source_epoch.unwrap_or(crate::epoch::CURRENT_EPOCH);
    analyzer.set_immutable_by_default(effective_epoch >= 8);
    analyzer.set_unicode_generation(cctx.unicode_generation());
    analyzer.bind_primitives(&meta);
    if !expander.core_env.is_empty() {
        analyzer.bind_compile_time_env(&expander.core_env, true);
    }
    let mut hir = analyzer.analyze_file_letrec(forms, span)?;
    let prim_values = analyzer.primitive_values().clone();
    let signal_projection = analyzer.take_signal_projection();
    let errors = analyzer.take_errors();
    drop(analyzer);

    if !errors.is_empty() {
        let err = &errors[0];
        let msg = match &err.location {
            Some(loc) => format!(
                "{}:{}:{}: {}",
                loc.file,
                loc.line,
                loc.col,
                err.description()
            ),
            None => err.description(),
        };
        return Err(msg);
    }

    let (dispatch_wrappers, fn_inline) = cctx.compile_registries_mut();
    crate::hir::regularize(&mut hir, &mut arena, symbols, dispatch_wrappers, fn_inline)?;

    Ok((hir, arena, expander, prim_values, signal_projection))
}
