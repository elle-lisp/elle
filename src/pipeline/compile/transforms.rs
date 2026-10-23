// audited: 2026-10-21
//! The source transforms the test-runner compile modes apply to a file's forms.
//!
//! docs/test-runner.md
//! src/pipeline/AGENTS.md

use super::*;
use crate::syntax::{ScopeId, SyntaxArena};

/// The fault-barrier transform (see `compile_barrier_module`). Runs on the
/// macro-EXPANDED top-level forms — so binding macros like `defn`/`def-` have
/// already become `def`/`var`, and are recognized as setup rather than wrapped:
///
/// ```text
/// prepend   (def __test-out (@array))
/// def/var   → unchanged (eager; establishes the shared binding)
/// signal    → unchanged (metadata; no entry)
/// expr E    → (push __test-out (array IDX (fn [] E)))   ; thunk, NOT run here
/// append    __test-out                                   ; module return value
/// ```
///
/// `IDX` is the form's position among the post-epoch, post-expansion forms,
/// which aligns with the runner's `read-all` index (used for label/hash/src).
/// Each test thunk is a closure capturing the shared bindings; it is created
/// (but not invoked) when the module runs on the bytecode tier, then run by the
/// runner on each tier. Setup forms run eagerly, so a later test form — and a
/// later thunk — sees earlier defs.
pub(super) fn barrier_transform(
    arena: &SyntaxArena,
    expanded: Vec<Syntax>,
    acc_scope: ScopeId,
) -> Vec<Syntax> {
    let sp = Span::synthetic();
    let sym = |s: &str| Syntax::symbol(arena, s, sp);
    let slist = |items: Vec<Syntax>| Syntax::list(arena, &items, sp);
    // The accumulator is compiler-injected, not written by the user. Scope-stamp
    // every occurrence with a fresh scope, so it is *hygienic*: a reference the
    // user writes (which never carries this scope) can never resolve to it. So
    // `(environment)` excludes it, by the same scope-subset rule that hides
    // macro-introduced bindings, and a test file that binds its own
    // `__test-out` does not collide with the harness.
    let acc = |s: &str| Syntax::symbol_scoped(arena, s, sp, &[acc_scope]);
    let acc_name = "__test-out";

    let mut out: Vec<Syntax> = Vec::with_capacity(expanded.len() + 2);
    // (def __test-out (@array))
    out.push(slist(vec![
        sym("def"),
        acc(acc_name),
        slist(vec![sym("@array")]),
    ]));

    for (idx, form) in expanded.into_iter().enumerate() {
        let is_setup = matches!(
            classify_form(&form),
            FileForm::Def(..) | FileForm::Var(..) | FileForm::Signal(..)
        );
        if is_setup {
            out.push(form);
        } else {
            // (fn [] E)
            let thunk = slist(vec![sym("fn"), Syntax::array(arena, &[], sp), form]);
            // (array IDX thunk)
            let pair = slist(vec![
                sym("array"),
                Syntax::new(SyntaxKind::Int(idx as i64), sp),
                thunk,
            ]);
            // (push __test-out pair)
            out.push(slist(vec![sym("push"), acc(acc_name), pair]));
        }
    }
    // __test-out  (the letrec body → the module's return value)
    out.push(acc(acc_name));
    out
}

/// The whole-file transform (see `compile_whole_module`). Runs on the
/// macro-EXPANDED top-level forms and wraps **all** of them, `def`/`var` setup
/// and expressions alike, into the body of a single 0-arg thunk, returned as
/// one `[0 thunk]` entry. This is the legacy multi-form path. A file is an
/// imperative script whose forms depend on their order, so it is run as one
/// unit: in source order, once per tier, in isolation. Sliced into per-form
/// thunks with `def`/`var` hoisted eagerly, it would run reordered
/// (read-before-write) and re-run shared mutations per tier.
///
/// The thunk body is the internal `(%file-body form…)` form, NOT a bare
/// `(fn () form…)` body. A plain `fn` body uses internal-define semantics: all
/// defines hoisted into one frame, last-wins shadowing. `%file-body` runs the
/// exact `analyze_file_letrec` a real file gets. So forward references, mutual
/// recursion, AND def redefinition (a redefinition's RHS sees the previous
/// binding) resolve identically to a direct `elle FILE` run. Output shape
/// matches `barrier_transform` (a `__test-out` array of `[index thunk]`), so the
/// runner consumes both the same way:
///
/// ```text
/// (def __test-out (@array))
/// (push __test-out (array 0 (fn () (%file-body form1 form2 … formN))))
/// __test-out
/// ```
pub(super) fn whole_module_transform(
    arena: &SyntaxArena,
    expanded: Vec<Syntax>,
    acc_scope: ScopeId,
) -> Vec<Syntax> {
    let sp = Span::synthetic();
    let sym = |s: &str| Syntax::symbol(arena, s, sp);
    let slist = |items: Vec<Syntax>| Syntax::list(arena, &items, sp);
    // Hygienic accumulator symbol (see `barrier_transform`).
    let acc = |s: &str| Syntax::symbol_scoped(arena, s, sp, &[acc_scope]);
    let acc_name = "__test-out";

    // (fn () (%file-body form1 … formN)) — all forms become the thunk body,
    // analyzed with file-scope letrec semantics. An empty file yields a thunk
    // whose body is nil.
    let body: Syntax = if expanded.is_empty() {
        sym("nil")
    } else {
        let mut fb: Vec<Syntax> = vec![sym("%file-body")];
        fb.extend(expanded);
        slist(fb)
    };
    let thunk = slist(vec![sym("fn"), Syntax::array(arena, &[], sp), body]);

    vec![
        // (def __test-out (@array))
        slist(vec![sym("def"), acc(acc_name), slist(vec![sym("@array")])]),
        // (push __test-out (array 0 thunk))
        slist(vec![
            sym("push"),
            acc(acc_name),
            slist(vec![
                sym("array"),
                Syntax::new(SyntaxKind::Int(0), sp),
                thunk,
            ]),
        ]),
        // __test-out  (the module's return value)
        acc(acc_name),
    ]
}

/// Compile a file as a single whole-file thunk (legacy multi-form path). Mirrors
/// `compile_barrier_module` but with `whole_module_transform`: the result is one
/// `[0 thunk]` entry whose thunk runs every form in source order
/// (docs/test-runner.md).
pub fn compile_whole_module(
    source: &str,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
    source_name: &str,
) -> Result<CodeUnit, String> {
    compile_module_with_transform(source, symbols, cctx, source_name, whole_module_transform)
}

/// Compile already-read top-level `syntaxes` as a single whole-file thunk
/// (`compile_whole_module` from syntax instead of source text). Powers
/// `compile/whole-module-syntax`: the test runner parses a legacy multi-form
/// file once in the main VM, and ships the syntax to a worker. The worker
/// compiles and runs it with its own stdlib, so the file's runtime `import`s
/// and the worker's `ev/run` scheduler agree on the dynamic scheduler
/// parameters.
pub fn compile_whole_module_forms(
    syntaxes: Vec<Syntax>,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
    source_name: &str,
) -> Result<CodeUnit, String> {
    compile_syntaxes_with_transform(syntaxes, symbols, cctx, source_name, whole_module_transform)
}
