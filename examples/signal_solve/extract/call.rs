// audited: 2026-10-21
//! How a call becomes facts: its arguments, its callee's variable, and the calls the analyzer leaves uncharged.
//!
//! docs/impl/solver.md

use super::{AVal, Extractor, PendingExportCall};
use crate::model::{bit_positions, SiteId, VarId, VarKey};
use elle::hir::Hir;

impl<'a> Extractor<'a> {
    pub(super) fn call(
        &mut self,
        hir: &'a Hir,
        func: &'a Hir,
        args: &'a [elle::hir::CallArg],
        ctx: VarId,
    ) {
        let site = self.site(hir.id);
        let nargs = args.len() as u32;
        for (i, a) in args.iter().enumerate() {
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
                let owner = self.owner(v);
                self.use_fact(ctx, site, v, owner, nargs);
            }
            AVal::ParamVal(p) => {
                if let Some(bound) = self.bounds.get(&p).copied() {
                    for bit in bit_positions(bound.bits.raw()) {
                        self.model.facts.raw.insert((ctx, bit));
                    }
                } else {
                    self.use_fact(ctx, site, p, p, nargs);
                }
            }
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

    /// The variable whose parameters a call of `v` binds.
    fn owner(&self, v: VarId) -> VarId {
        match self.model.keys_by_id[v as usize] {
            VarKey::Sq(_) => self
                .model
                .facts
                .sqof
                .iter()
                .find(|(q, _)| *q == v)
                .map(|(_, f)| self.owner(*f))
                .unwrap_or(v),
            _ => v,
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
