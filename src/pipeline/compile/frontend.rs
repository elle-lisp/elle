// audited: 2026-10-21
//! The file front end: source text or read forms to regularized HIR, shared by every file entry point but `compile_file_to_lir`.
//!
//! src/pipeline/AGENTS.md
//! docs/pipeline.md
//!
//! The stages run in order: read, strip declarations, migrate, expand with
//! include splicing, transform, classify, analyze and regularize.

use super::*;

/// What the file front end produces, for `compile_file`, the FHIR entry
/// points and the test-mode entry points alike.
///
/// A caller that does not need a field ignores it.
pub(super) struct Frontend {
    pub hir: crate::hir::Hir,
    pub arena: BindingArena,
    pub expander: crate::syntax::Expander,
    pub prim_values: std::collections::HashMap<crate::hir::Binding, crate::value::Value>,
    pub signal_projection: Option<std::collections::HashMap<String, crate::signals::Signal>>,
    /// The inferred types, which the lowerer reads to mark an operation whose
    /// operands the intrinsic operand contract proved are integers
    /// (docs/impl/lir.md).
    pub types: crate::hir::TypeInfo,
}

pub(super) type FrontendResult = Result<Frontend, String>;

/// The front end for file compilation: read, epoch migration, macro expansion
/// with file-scope stamping and include resolution, analysis, which fails with
/// the first error it collected, and `regularize`.
pub(super) fn compile_file_frontend(
    source: &str,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
    source_name: &str,
) -> FrontendResult {
    compile_file_frontend_xform(
        source,
        symbols,
        cctx,
        source_name,
        |_arena, forms, _scope| forms,
    )
}

/// Like `compile_file_frontend`, but applies `xform` to the macro-expanded
/// top-level forms before classification and analysis. The fault-barrier test
/// mode (`compile_barrier_module`) uses this to wrap each test form in a
/// captured thunk and leave the `def` and `var` setup forms intact. The file
/// then still takes the whole-module `analyze_file_letrec` path, which
/// per-form `eval` cannot.
///
/// Reads and compiles inside the unit's syntax arena, which
/// `with_syntax_arena` frees on return.
pub(super) fn compile_file_frontend_xform(
    source: &str,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
    source_name: &str,
    xform: impl FnOnce(&SyntaxArena, Vec<Syntax>, crate::syntax::ScopeId) -> Vec<Syntax>,
) -> FrontendResult {
    with_syntax_arena(cctx.heap_ptr(), |arena| {
        let t = std::time::Instant::now();
        let syntaxes = read_syntax_all_for(arena, source, source_name)?;
        crate::phase!(
            crate::trace::compile(),
            "compile",
            t,
            "{} read",
            source_name
        );
        crate::epoch::check_lexicon_agreement(&syntaxes, source, source_name)?;
        compile_syntaxes_frontend_xform_inner(arena, syntaxes, symbols, cctx, source_name, xform)
    })
}

/// Like `compile_file_frontend_xform`, but starts from already-read top-level
/// `syntaxes` instead of source text, so a caller can read once and compile
/// the forms elsewhere. The test runner reads a multi-form file in the main VM
/// and ships the forms to a worker. The worker compiles and runs them with its
/// own stdlib, so the file's runtime `import`s and the worker's `ev/run`
/// scheduler share one set of dynamic parameters. Every later stage is the
/// text path's.
pub(super) fn compile_syntaxes_frontend_xform(
    syntaxes: Vec<Syntax>,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
    source_name: &str,
    xform: impl FnOnce(&SyntaxArena, Vec<Syntax>, crate::syntax::ScopeId) -> Vec<Syntax>,
) -> FrontendResult {
    with_syntax_arena(cctx.heap_ptr(), |arena| {
        // The incoming forms live in the regions of the `Value`s that carried
        // them here. Copy them into this unit's working arena, so the unit
        // owns its own tree and nothing it builds points at a value that may
        // be released while it compiles.
        let syntaxes: Vec<Syntax> = syntaxes.iter().map(|s| s.copy_into(&arena)).collect();
        compile_syntaxes_frontend_xform_inner(arena, syntaxes, symbols, cctx, source_name, xform)
    })
}

/// The front end past reading: migrate, expand, `xform`, classify, analyze and
/// regularize. Every caller runs it inside `with_syntax_arena`, so the
/// per-compilation scratch is freed when the caller returns.
#[allow(clippy::type_complexity)]
fn compile_syntaxes_frontend_xform_inner(
    arena: SyntaxArena,
    mut syntaxes: Vec<Syntax>,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
    source_name: &str,
    xform: impl FnOnce(&SyntaxArena, Vec<Syntax>, crate::syntax::ScopeId) -> Vec<Syntax>,
) -> FrontendResult {
    let ct = crate::trace::compile();
    let t = std::time::Instant::now();

    let source_epoch = crate::epoch::extract_epoch(&mut syntaxes)?;
    // Version and migration declarations vanish before migration, so an
    // epoch rewrite never reaches inside one (docs/versioning.md).
    crate::pipeline::directives::extract_semver_directives(&mut syntaxes)?;
    if let Some(epoch) = source_epoch {
        crate::epoch::migrate_forms(&arena, &mut syntaxes, epoch)?;
    }

    let (expanded_forms, mut expander, meta) =
        cctx.with_macro_expansion(arena, |macro_vm, mut expander, meta| {
            // A form takes the file scope as it leaves the queue, after the
            // include splice, so a spliced form shares the including file's
            // scope and each sees what the other defines (docs/modules.md).
            let file_scope = (!source_name.starts_with('<')).then(|| expander.fresh_scope());
            let mut pending: std::collections::VecDeque<Syntax> = syntaxes.into();
            let mut expanded_forms = Vec::new();
            let mut included = included_root(source_name);
            while let Some(syntax) = pending.pop_front() {
                if resolve_and_splice_include(
                    &arena,
                    &syntax,
                    macro_vm,
                    &meta,
                    &mut pending,
                    &mut included,
                )? {
                    continue;
                }
                let syntax = match file_scope {
                    Some(scope) => expander.stamp_scope(syntax, scope),
                    None => syntax,
                };
                expanded_forms.push(expander.expand(syntax, symbols, macro_vm)?);
            }
            Ok::<_, String>((expanded_forms, expander, meta))
        })?;
    crate::phase!(ct, "compile", t, "{} expand", source_name);
    let t = std::time::Instant::now();

    // A fresh scope for any accumulator/temporaries `xform` injects, minted after
    // expansion so it cannot collide with a scope the expander already assigned.
    // Stamping the injected symbols with it makes them hygienic (invisible to
    // user references and to `(environment)`); the identity xform ignores it.
    let acc_scope = expander.fresh_scope();
    let expanded_forms = xform(&arena, expanded_forms, acc_scope);

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

    let types = crate::hir::regularize(&mut hir, &mut arena, symbols, &prim_values, cctx.graft())?;
    crate::phase!(ct, "compile", t, "{} analyze", source_name);

    Ok(Frontend {
        hir,
        arena,
        expander,
        prim_values,
        signal_projection,
        types,
    })
}
