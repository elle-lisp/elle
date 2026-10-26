// audited: 2026-10-24
//! How a call becomes facts: its arguments, its callee's variable, and the calls the analyzer leaves uncharged.
//!
//! docs/impl/solver.md

use super::{AVal, CallSite, Extractor, PendingExportCall};
use crate::hir::solve::model::{bit_positions, SiteId, VarId, VarKey};
use crate::hir::{CallArg, Hir, HirKind};
use crate::value::fiber::SignalBits;

impl<'a> Extractor<'a> {
    pub(super) fn call(&mut self, hir: &'a Hir, func: &'a Hir, args: &'a [CallArg], ctx: VarId) {
        let site = self.site(hir.id);
        self.calls.insert(hir.id, CallSite { site, ctx });
        // A spliced argument binds positions nobody knows, so every parameter
        // from the first splice on binds nothing known.
        let nargs = args.iter().position(|a| a.spliced).unwrap_or(args.len()) as u32;
        for (i, a) in args.iter().take(nargs as usize).enumerate() {
            let v = self.aval(&a.expr);
            let v = self.var_of(v);
            self.model.facts.arg.insert((site, i as u32, v));
        }
        let callee = self.aval(func);
        // A collection literal and a qualified name build a call whose signal
        // the analyzer never charges to the enclosing function; the :error
        // such a form may raise arrives with the function's own bits. Both give
        // the callee the span of the whole form, where a written call's callee
        // has its own.
        // A form the compiler builds, such as `import-file`'s load, spans its
        // callee the same way and is charged, so the span alone cannot tell.
        if func.span == hir.span && UNCHARGED.iter().any(|p| self.is_prim_named(func, p)) {
            return;
        }
        // A call of `emit` whose first argument is a literal raises what the
        // literal names, as the analyzer charges it.
        if let Some(bits) = self.emit_bits(func, args) {
            for bit in bit_positions(bits.raw()) {
                self.model.facts.raw.insert((ctx, bit));
            }
            return;
        }
        match callee {
            AVal::Callable(v) => {
                if let VarKey::Inst(..) = self.model.keys_by_id[v as usize] {
                    self.model.site_callee.insert(site, v);
                    self.pending.export_calls.push(PendingExportCall {
                        ctx,
                        site,
                        callee: v,
                        nargs,
                    });
                    return;
                }
                let owner = self.model.owner(v);
                self.use_fact(ctx, site, v, owner, nargs);
            }
            AVal::ParamVal(p) => self.use_fact(ctx, site, p, p, nargs),
            AVal::ModuleRef { path, .. } => {
                self.model.site_nargs.insert(site, nargs);
                self.pending.instantiations.push((ctx, site, path, nargs));
            }
            _ => {
                let u = self.model.unknown();
                self.use_fact(ctx, site, u, u, nargs);
            }
        }
    }

    /// The bits a call of the `emit` primitive raises when its first argument
    /// names them: an integer's bits, or a registered keyword's bit with
    /// `:yield`, which keeps the call resumable.
    fn emit_bits(&self, func: &Hir, args: &[CallArg]) -> Option<SignalBits> {
        if !self.is_prim_named(func, "emit") {
            return None;
        }
        match &args.first()?.expr.kind {
            HirKind::Int(bits) => Some(SignalBits::from_i64(*bits)),
            HirKind::Keyword(kw) => {
                let reg = crate::signals::registry::global_registry()
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                let pos = reg.lookup(kw)?;
                Some(SignalBits::from_bit(pos).union(crate::signals::SIG_YIELD))
            }
            _ => None,
        }
    }

    fn use_fact(&mut self, ctx: VarId, site: SiteId, callee: VarId, owner: VarId, nargs: u32) {
        self.model.site_callee.insert(site, callee);
        self.model.use_fact(ctx, site, callee, owner, nargs);
    }
}

/// The primitives a collection literal or a qualified name calls. The
/// analyzer's `analyze_collection_literal`, `meta/location` and the `a:b`
/// desugaring build these calls without charging their signal. They report
/// what the form may raise as the enclosing function's own bits.
const UNCHARGED: &[&str] = &[
    "array", "@array", "bytes", "@bytes", "struct", "@struct", "get",
];
