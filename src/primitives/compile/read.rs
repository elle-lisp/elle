// audited: 2026-11-20
//! `compile/read-forms`: a file's forms as a compile or a module loader sees them, read and migrated but not expanded.
//!
//! docs/modules.md
//! docs/test-runner.md

use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::Value;

/// `(compile/read-forms source name [:module])` — parse SOURCE into a list of
/// syntax values (spans preserved), without expanding or compiling. The forms
/// come back as a file compile sees them: the `(elle/epoch N)` line and the
/// version declarations removed, and every form migrated from that epoch to
/// the current one. With `:module`, they migrate as a module's, so an old
/// module of several forms folds into one (docs/epochs.md).
///
/// The test runner reads a multi-form file once in the main VM with this. It
/// ships the syntax to a worker over `os/spawn`, and the worker compiles it
/// with `compile/whole-module-syntax` against its own stdlib. The file's
/// runtime `import`s and the worker's `ev/run` scheduler then share one set of
/// dynamic parameters. Reading needs no symbol table, so this answers directly
/// (no VM dispatch).
pub(super) fn prim_compile_read_forms(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let mut strings = Vec::with_capacity(2);
    for (arg, label) in args.iter().zip(["source", "name"]) {
        match arg.with_string(|s| s.to_string()) {
            Some(s) => strings.push(s),
            None => {
                let message = format!(
                    "compile/read-forms: {label} must be a string, got {}",
                    arg.type_name()
                );
                return (SIG_ERROR, ctx.error("type-error", message));
            }
        }
    }
    let (source, name) = (&strings[0], &strings[1]);
    let module = match args.get(2) {
        None => false,
        Some(arg) if arg.is_keyword_named("module") => true,
        Some(arg) => {
            let message = format!(
                "compile/read-forms: the third argument must be :module, got {}",
                arg.type_name()
            );
            return (SIG_ERROR, ctx.error("type-error", message));
        }
    };
    // Read into the call's region; each `ctx.syntax` wrapper then owns its
    // own copy of the form it wraps.
    let arena = ctx.syntax_arena();
    let read = crate::reader::read_syntax_all_for(arena, source, name).and_then(|mut forms| {
        crate::epoch::check_lexicon_agreement(&forms, source, name)?;
        let epoch = crate::epoch::extract_epoch(&mut forms)?;
        // Version and migration declarations leave before migration, as a
        // file compile's do (docs/versioning.md).
        crate::pipeline::directives::extract_semver_directives(&mut forms)?;
        match epoch {
            Some(epoch) if module => crate::epoch::migrate_module_forms(&arena, &mut forms, epoch)?,
            Some(epoch) => crate::epoch::migrate_forms(&arena, &mut forms, epoch)?,
            None => 0,
        };
        Ok(forms)
    });
    match read {
        Ok(forms) => {
            let vals: Vec<Value> = forms.into_iter().map(|s| ctx.syntax(s)).collect();
            (SIG_OK, ctx.list(vals))
        }
        Err(e) => (SIG_ERROR, ctx.error("compile-error", e)),
    }
}
