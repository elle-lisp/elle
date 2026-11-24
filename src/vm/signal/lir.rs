// audited: 2026-11-21
//! The `compile/lir` query: a unit compiled from source, answered in the data form.
//!
//! docs/spec/form.md

use super::*;

impl VM {
    /// `(compile/lir source name)` — compile a unit through the file front
    /// end, as `compile/dumps` does, and answer each of its functions as a
    /// struct in the data form.
    ///
    /// The registry comes back as it was. A signal the source declares is
    /// named while the answer is built, and spends no user bit after it.
    pub(super) fn dispatch_compile_lir(
        &mut self,
        ctx: &mut crate::primitives::ctx::Alloc,
        arg: Value,
    ) -> (SignalBits, Value) {
        let prim = "compile/lir";
        let (source, name) = match Self::source_and_name(ctx, arg, prim) {
            Ok(pair) => pair,
            Err(refusal) => return refusal,
        };
        let baseline = crate::signals::registry::snapshot_registry();
        let answer = self.with_compiler(ctx, prim, |symbols, cctx| {
            let module = crate::pipeline::compile_file_to_lir(&source, symbols, cctx, &name, 0)?;
            Ok::<_, String>(crate::dump::form::unit(ctx, symbols, &module))
        });
        crate::signals::registry::restore_registry(baseline);
        match answer {
            Ok(Ok(unit)) => (SIG_OK, unit),
            Ok(Err(msg)) => (SIG_ERROR, ctx.error("compile-error", msg)),
            Err(refusal) => refusal,
        }
    }
}
