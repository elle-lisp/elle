// audited: 2026-10-06
//! The abstract value of an expression: what the extractor knows a callee, an argument or an export may be.
//!
//! docs/impl/solver.md

use super::{literal_mask, AVal, Extractor};
use crate::model::{bit_positions, VarId, VarKey};
use crate::visit::resolve;
use elle::hir::{Binding, BindingScope, Hir, HirKind};

impl<'a> Extractor<'a> {
    /// The abstract value of an expression.
    pub fn aval(&mut self, hir: &'a Hir) -> AVal {
        match &hir.kind {
            HirKind::Lambda { .. } => AVal::Callable(self.lam_var(hir.id)),
            HirKind::Nil
            | HirKind::EmptyList
            | HirKind::Bool(_)
            | HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::String(_)
            | HirKind::Keyword(_)
            | HirKind::Quote(_)
            | HirKind::QuoteConst(_) => AVal::Data,
            HirKind::Var(b) => self.binding_aval(*b),
            HirKind::Begin(exprs) => match exprs.last() {
                Some(e) => self.aval(e),
                None => AVal::Unknown,
            },
            HirKind::Let { body, .. } | HirKind::Letrec { body, .. } => self.aval(body),
            HirKind::Call { func, args, .. } => self.call_aval(hir, func, args),
            _ => AVal::Unknown,
        }
    }

    fn binding_aval(&mut self, b: Binding) -> AVal {
        let inner = self.unit.arena.get(b);
        if inner.is_mutated {
            return AVal::Unknown;
        }
        if inner.is_primitive {
            return AVal::Callable(self.prim(b));
        }
        if let Some(&p) = self.param_of.get(&b) {
            return AVal::ParamVal(p);
        }
        if inner.scope == BindingScope::Parameter {
            return AVal::Unknown;
        }
        if let Some(v) = self.memo.get(&b) {
            return v.clone();
        }
        if !self.busy.insert(b) {
            return AVal::Unknown;
        }
        let v = if let Some(init) = self.inits.get(&b).copied() {
            self.aval(init)
        } else if let Some((value, key)) = self.destructured.get(&b).cloned() {
            let base = self.aval(value);
            self.field(base, &key)
        } else {
            AVal::Unknown
        };
        self.busy.remove(&b);
        self.memo.insert(b, v.clone());
        v
    }

    fn call_aval(&mut self, hir: &'a Hir, func: &'a Hir, args: &'a [elle::hir::CallArg]) -> AVal {
        if self.is_prim_named(func, "import/load-file") {
            if let Some(HirKind::String(literal)) = args.first().map(|a| &a.expr.kind) {
                if let Some(path) = resolve(literal) {
                    let site = self.site(hir.id);
                    return AVal::ModuleRef { path, site };
                }
            }
            return AVal::Unknown;
        }
        if self.is_prim_named(func, "get") && args.len() == 2 {
            if let HirKind::Keyword(k) = &args[1].expr.kind {
                let base = self.aval(&args[0].expr);
                return self.field(base, k);
            }
        }
        if self.is_prim_named(func, "struct") {
            let mut fields = Vec::new();
            for pair in args.chunks(2) {
                if let [k, v] = pair {
                    if let HirKind::Keyword(k) = &k.expr.kind {
                        let v = self.aval(&v.expr);
                        fields.push((k.clone(), v));
                    }
                }
            }
            return AVal::Struct(fields);
        }
        let squelch = self.is_prim_named(func, "squelch");
        let attune = self.is_prim_named(func, "attune");
        if (squelch || attune) && args.len() == 2 {
            let (target, mask) = if squelch {
                (&args[0].expr, &args[1].expr)
            } else {
                (&args[1].expr, &args[0].expr)
            };
            if let Some(mask) = literal_mask(mask) {
                let mask = if attune {
                    elle::signals::CAP_MASK.raw() & !mask
                } else {
                    mask
                };
                let site = self.site(hir.id);
                let q = self.model.var(VarKey::Sq(site));
                let t = self.aval(target);
                let f = self.var_of(t);
                self.model.facts.sqof.insert((q, f));
                for bit in bit_positions(mask) {
                    self.model.facts.sqm.insert((q, bit));
                }
                return AVal::Callable(q);
            }
        }
        if let AVal::ModuleRef { path, .. } = self.aval(func) {
            let site = self.site(hir.id);
            self.model.site_nargs.insert(site, args.len() as u32);
            return AVal::Instance { path, site };
        }
        AVal::Unknown
    }

    /// Field `key` of a value.
    fn field(&mut self, base: AVal, key: &str) -> AVal {
        match base {
            AVal::Instance { path, site } => {
                self.pending.instances.push((site, path, false));
                let k = self.model.key(key);
                AVal::Callable(self.model.var(VarKey::Inst(site, k)))
            }
            AVal::ModuleRef { path, site } => {
                self.pending.instances.push((site, path, true));
                let k = self.model.key(key);
                AVal::Callable(self.model.var(VarKey::Inst(site, k)))
            }
            AVal::ParamVal(p) => {
                let k = self.model.key(key);
                AVal::Callable(self.model.var(VarKey::Field(p, k)))
            }
            AVal::Struct(fields) => fields
                .into_iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v)
                .unwrap_or(AVal::Unknown),
            _ => AVal::Unknown,
        }
    }

    /// The variable an argument or an export stands for.
    pub fn var_of(&mut self, v: AVal) -> VarId {
        match v {
            AVal::Callable(v) | AVal::ParamVal(v) => v,
            AVal::Data => self.model.var(VarKey::Data),
            AVal::Instance { path, site } => {
                self.pending.instances.push((site, path, false));
                self.model.var(VarKey::Obj(site))
            }
            _ => self.model.unknown(),
        }
    }
}
