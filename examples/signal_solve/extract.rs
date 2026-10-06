// audited: 2026-10-06
//! Walk one file's analyzed HIR and emit the facts the solver reads.
//!
//! docs/impl/solver.md
//! docs/modules-proposal.md

mod shape;
mod value;

pub use shape::ModuleShape;

use crate::model::{bit_positions, FileId, Model, SiteId, VarId, VarKey};
use crate::visit::each_child;
use elle::hir::{Binding, BindingArena, Hir, HirId, HirKind, HirPattern, LambdaDecl, PatternKey};
use elle::signals::Signal;
use elle::value::SymbolId;
use std::collections::{HashMap, HashSet};

/// The abstract value of an expression: what the solver knows it may be.
#[derive(Debug, Clone)]
pub enum AVal {
    /// A callable with a signal variable.
    Callable(VarId),
    /// A parameter: callable, or a struct whose fields are free.
    ParamVal(VarId),
    /// The value of a literal `(import-file "path")`, before it is called.
    ModuleRef {
        path: String,
        site: SiteId,
    },
    /// The value a call of a module's lambda returned.
    Instance {
        path: String,
        site: SiteId,
    },
    /// A literal datum.
    Data,
    /// A struct literal.
    Struct(Vec<(String, AVal)>),
    Unknown,
}

/// A call whose callee is a module export: its parameters belong to the
/// export, which only the link step knows.
pub struct PendingExportCall {
    pub ctx: VarId,
    pub site: SiteId,
    pub callee: VarId,
    pub nargs: u32,
}

/// What a file's own extraction leaves for the link step.
#[derive(Default)]
pub struct Pending {
    /// `(ctx, site, path, nargs)`: a call of a literal `(import-file "path")`'s lambda.
    pub instantiations: Vec<(VarId, SiteId, String, u32)>,
    /// An instance or a struct module, and its path, for every site whose
    /// fields were read.
    pub instances: Vec<(SiteId, String, bool)>,
    pub export_calls: Vec<PendingExportCall>,
}

/// One analyzed file.
pub struct Unit {
    pub id: FileId,
    pub path: String,
    pub hir: Hir,
    pub arena: BindingArena,
    pub decls: HashMap<HirId, LambdaDecl>,
}

pub struct Extractor<'a> {
    pub model: &'a mut Model,
    meta: &'a HashMap<SymbolId, Signal>,
    unit: &'a Unit,
    inits: HashMap<Binding, &'a Hir>,
    destructured: HashMap<Binding, (&'a Hir, String)>,
    param_of: HashMap<Binding, VarId>,
    bounds: HashMap<VarId, Signal>,
    memo: HashMap<Binding, AVal>,
    busy: HashSet<Binding>,
    sites: HashMap<HirId, SiteId>,
    pub pending: Pending,
    /// The binding name of each lambda bound by a `def`, `let` or `letrec`.
    pub lambda_names: HashMap<HirId, SymbolId>,
}

impl<'a> Extractor<'a> {
    pub fn new(model: &'a mut Model, meta: &'a HashMap<SymbolId, Signal>, unit: &'a Unit) -> Self {
        let mut ex = Extractor {
            model,
            meta,
            unit,
            inits: HashMap::new(),
            destructured: HashMap::new(),
            param_of: HashMap::new(),
            bounds: HashMap::new(),
            memo: HashMap::new(),
            busy: HashSet::new(),
            sites: HashMap::new(),
            pending: Pending::default(),
            lambda_names: HashMap::new(),
        };
        ex.index(&unit.hir);
        ex
    }

    pub fn lam_var(&mut self, id: HirId) -> VarId {
        self.model.var(VarKey::Lam(self.unit.id, id.0))
    }

    fn site(&mut self, id: HirId) -> SiteId {
        if let Some(&s) = self.sites.get(&id) {
            return s;
        }
        let s = self.model.site();
        self.sites.insert(id, s);
        s
    }

    fn name(&self, b: Binding) -> SymbolId {
        self.unit.arena.get(b).name
    }

    fn is_prim_named(&self, func: &Hir, name: &str) -> bool {
        matches!(&func.kind, HirKind::Var(b)
            if self.unit.arena.get(*b).is_primitive && self.name(*b) == SymbolId::of(name))
    }

    /// Record every binding's initializer, every parameter's owner, and every
    /// bound lambda's name, before any value is asked for.
    fn index(&mut self, hir: &'a Hir) {
        match &hir.kind {
            HirKind::Lambda {
                params,
                param_bounds,
                body,
                ..
            } => {
                let lam = self.lam_var(hir.id);
                self.model.nparams.insert(lam, params.len() as u32);
                for (i, p) in params.iter().enumerate() {
                    let pv = self.model.var(VarKey::Param(lam, i as u32));
                    self.param_of.insert(*p, pv);
                }
                for pb in param_bounds {
                    let pv = self.param_of[&pb.binding];
                    self.bounds.insert(pv, pb.signal);
                }
                self.index(body);
            }
            HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                for (b, init) in bindings {
                    self.inits.insert(*b, init);
                    if matches!(init.kind, HirKind::Lambda { .. }) {
                        self.lambda_names.insert(init.id, self.name(*b));
                    }
                    self.index(init);
                }
                self.index(body);
            }
            HirKind::Define { binding, value } => {
                self.inits.insert(*binding, value);
                if matches!(value.kind, HirKind::Lambda { .. }) {
                    self.lambda_names.insert(value.id, self.name(*binding));
                }
                self.index(value);
            }
            HirKind::Destructure { pattern, value, .. } => {
                self.index_pattern(pattern, value);
                self.index(value);
            }
            _ => each_child(hir, &mut |c| self.index(c)),
        }
    }

    fn index_pattern(&mut self, pattern: &'a HirPattern, value: &'a Hir) {
        let entries = match pattern {
            HirPattern::Struct { entries, .. }
            | HirPattern::Table { entries, .. }
            | HirPattern::NamedStruct { entries } => entries,
            _ => return,
        };
        for (key, sub) in entries {
            if let (PatternKey::Keyword(k), HirPattern::Var(b)) = (key, sub) {
                self.destructured.insert(*b, (value, k.clone()));
            }
        }
    }

    /// The variable of a primitive binding, with its bits and the parameters
    /// it propagates.
    fn prim(&mut self, b: Binding) -> VarId {
        let sym = self.name(b);
        let key = VarKey::Prim(sym.0);
        let fresh = !self.model.has(&key);
        let v = self.model.var(key);
        if fresh {
            let sig = self.meta.get(&sym).copied().unwrap_or(Signal::unknown());
            for bit in bit_positions(sig.bits.raw()) {
                self.model.facts.bits.insert((v, bit));
            }
            let mut n = 0;
            for i in sig.propagated_params() {
                let p = self.model.var(VarKey::Param(v, i as u32));
                self.model.facts.dep.insert((v, p));
                n = n.max(i as u32 + 1);
            }
            self.model.nparams.insert(v, n);
        }
        v
    }

    /// Walk the file, emitting facts. `ctx` is the variable whose signal the
    /// code being walked contributes to.
    pub fn walk(&mut self, hir: &'a Hir, ctx: VarId) {
        match &hir.kind {
            HirKind::Lambda { body, .. } => {
                let lam = self.lam_var(hir.id);
                if let Some(decl) = self.unit.decls.get(&hir.id).copied() {
                    for bit in bit_positions(decl.muffle.raw()) {
                        self.model.facts.muf.insert((lam, bit));
                    }
                    if let Some(c) = decl.ceiling {
                        self.model.facts.ceiled.insert(lam);
                        for bit in bit_positions(c.bits.raw()) {
                            self.model.facts.ceil.insert((lam, bit));
                        }
                    }
                }
                self.walk(body, lam);
            }
            HirKind::Emit { signal, value } => {
                for bit in bit_positions(signal.raw()) {
                    self.model.facts.raw.insert((ctx, bit));
                }
                self.walk(value, ctx);
            }
            HirKind::Match { value, arms } => {
                let total = arms
                    .iter()
                    .any(|(p, g, _)| g.is_none() && p.is_irrefutable());
                if !total {
                    self.model.facts.raw.insert((ctx, 0));
                }
                self.walk(value, ctx);
                for (_, g, body) in arms {
                    if let Some(g) = g {
                        self.walk(g, ctx);
                    }
                    self.walk(body, ctx);
                }
            }
            HirKind::Call { func, args, .. } => {
                self.call(hir, func, args, ctx);
                self.walk(func, ctx);
                for a in args {
                    self.walk(&a.expr, ctx);
                }
            }
            _ => each_child(hir, &mut |c| self.walk(c, ctx)),
        }
    }

    fn call(&mut self, hir: &'a Hir, func: &'a Hir, args: &'a [elle::hir::CallArg], ctx: VarId) {
        let site = self.site(hir.id);
        let nargs = args.len() as u32;
        for (i, a) in args.iter().enumerate() {
            let v = self.aval(&a.expr);
            let v = self.var_of(v);
            self.model.facts.arg.insert((site, i as u32, v));
        }
        let callee = self.aval(func);
        // A collection literal and a qualified name build a call the analyzer
        // never charges to the enclosing function. Both give the callee the
        // span of the whole form, where a written call's callee has its own.
        if func.span == hir.span {
            if let AVal::Callable(v) = callee {
                if let VarKey::Prim(_) = self.model.keys_by_id[v as usize] {
                    return;
                }
            }
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

/// The bits a literal keyword or keyword-set mask names.
fn literal_mask(hir: &Hir) -> Option<u64> {
    let reg = elle::signals::registry::global_registry().lock().unwrap();
    match &hir.kind {
        HirKind::Keyword(k) => reg.to_signal_bits(k).map(|b| b.raw()),
        HirKind::Call { args, .. } => {
            let mut mask = 0u64;
            for a in args {
                match &a.expr.kind {
                    HirKind::Keyword(k) => mask |= reg.to_signal_bits(k)?.raw(),
                    _ => return None,
                }
            }
            Some(mask)
        }
        _ => None,
    }
}
