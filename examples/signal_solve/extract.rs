// audited: 2026-10-05
//! Walk one file's analyzed HIR and emit the facts the solver reads.
//!
//! docs/signals/inference.md
//! docs/modules.md

use crate::model::{bit_positions, FileId, Model, SiteId, VarId, VarKey};
use elle::hir::{
    Binding, BindingArena, BindingScope, Hir, HirId, HirKind, HirPattern, LambdaDecl, PatternKey,
};
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
    /// The value of `(import "literal")`, before it is called.
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
    /// `(ctx, site, path, nargs)`: a call of `(import "path")`'s lambda.
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

/// The shape a module file returns.
#[derive(Debug, Clone)]
pub enum ModuleShape {
    /// A lambda, with the export struct its body ends in.
    Lambda {
        lam: VarId,
        exports: Vec<(String, VarId)>,
    },
    /// A struct literal.
    Struct {
        exports: Vec<(String, VarId)>,
    },
    Unknown,
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
        if self.is_prim_named(func, "import") {
            if let Some(HirKind::String(spec)) = args.first().map(|a| &a.expr.kind) {
                if let Some(path) = resolve(spec) {
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
        use_fact(self.model, ctx, site, callee, owner, nargs);
    }

    /// The shape this file's module value has, and its exports.
    pub fn module_shape(&mut self) -> ModuleShape {
        let unit = self.unit;
        let HirKind::Letrec { bindings, .. } = &unit.hir.kind else {
            return ModuleShape::Unknown;
        };
        let Some((_, last)) = bindings.last() else {
            return ModuleShape::Unknown;
        };
        match &last.kind {
            HirKind::Lambda { body, .. } => {
                let lam = self.lam_var(last.id);
                let mut exports = Vec::new();
                self.export_struct(body, &mut exports);
                ModuleShape::Lambda { lam, exports }
            }
            _ => {
                let mut exports = Vec::new();
                self.export_struct(last, &mut exports);
                if exports.is_empty() {
                    ModuleShape::Unknown
                } else {
                    ModuleShape::Struct { exports }
                }
            }
        }
    }

    /// Collect the fields of the struct literal an expression ends in, through
    /// the forms `compute_signal_projection` unwraps.
    fn export_struct(&mut self, hir: &'a Hir, out: &mut Vec<(String, VarId)>) {
        match &hir.kind {
            HirKind::Call { func, args, .. } if self.is_prim_named(func, "struct") => {
                for pair in args.chunks(2) {
                    if let [k, v] = pair {
                        if let HirKind::Keyword(k) = &k.expr.kind {
                            let a = self.aval(&v.expr);
                            let var = self.var_of(a);
                            out.push((k.clone(), var));
                        }
                    }
                }
            }
            HirKind::Lambda { body, .. } => self.export_struct(body, out),
            HirKind::Begin(exprs) => {
                if let Some(e) = exprs.last() {
                    self.export_struct(e, out);
                }
            }
            HirKind::Let { body, .. } | HirKind::Letrec { body, .. } => {
                self.export_struct(body, out)
            }
            HirKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.export_struct(then_branch, out);
                self.export_struct(else_branch, out);
            }
            HirKind::Return { value } => self.export_struct(value, out),
            _ => {}
        }
    }
}

/// A path in the one spelling every table keys on.
pub fn canonical(path: &str) -> String {
    std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string())
}

/// Resolve an import spec as `import` does, to a `.lisp` file.
fn resolve(spec: &str) -> Option<String> {
    elle::primitives::modules::resolve_import(spec)
        .filter(|p| p.ends_with(".lisp"))
        .map(|p| canonical(&p))
}

/// The resolved `.lisp` paths of every `(import "literal")` in a file.
pub fn literal_imports(hir: &Hir, arena: &BindingArena, out: &mut Vec<String>) {
    if let HirKind::Call { func, args, .. } = &hir.kind {
        let is_import = matches!(&func.kind, HirKind::Var(b)
            if arena.get(*b).is_primitive && arena.get(*b).name == SymbolId::of("import"));
        if is_import {
            if let Some(HirKind::String(spec)) = args.first().map(|a| &a.expr.kind) {
                if let Some(path) = resolve(spec) {
                    if !out.contains(&path) {
                        out.push(path);
                    }
                }
            }
        }
    }
    each_child(hir, &mut |c| literal_imports(c, arena, out));
}

/// Emit `use(ctx, site, callee, owner)`, and `noarg` for each parameter of the
/// owner the call does not pass.
pub fn use_fact(
    model: &mut Model,
    ctx: VarId,
    site: SiteId,
    callee: VarId,
    owner: VarId,
    nargs: u32,
) {
    model.facts.uses.insert((ctx, site, callee, owner));
    let n = model.nparams.get(&owner).copied().unwrap_or(0);
    for i in nargs..n {
        model.facts.noarg.insert((site, i));
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

/// Visit every direct child expression of a node.
pub fn each_child<'h>(hir: &'h Hir, f: &mut dyn FnMut(&'h Hir)) {
    match &hir.kind {
        HirKind::Let { bindings, body }
        | HirKind::Letrec { bindings, body }
        | HirKind::Loop { bindings, body } => {
            for (_, v) in bindings {
                f(v);
            }
            f(body);
        }
        HirKind::Lambda { body, .. } => f(body),
        HirKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            f(cond);
            f(then_branch);
            f(else_branch);
        }
        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            for (c, b) in clauses {
                f(c);
                f(b);
            }
            if let Some(e) = else_branch {
                f(e);
            }
        }
        HirKind::Begin(es) | HirKind::And(es) | HirKind::Or(es) => es.iter().for_each(f),
        HirKind::Block { body, .. } => body.iter().for_each(f),
        HirKind::Break { value, .. }
        | HirKind::Return { value }
        | HirKind::Emit { value, .. }
        | HirKind::Assign { value, .. }
        | HirKind::Define { value, .. }
        | HirKind::Destructure { value, .. }
        | HirKind::MakeCell { value } => f(value),
        HirKind::Call { func, args, .. } => {
            f(func);
            for a in args {
                f(&a.expr);
            }
        }
        HirKind::While { cond, body } => {
            f(cond);
            f(body);
        }
        HirKind::Recur { args } | HirKind::Intrinsic { args, .. } => args.iter().for_each(f),
        HirKind::Match { value, arms } => {
            f(value);
            for (_, g, b) in arms {
                if let Some(g) = g {
                    f(g);
                }
                f(b);
            }
        }
        HirKind::Eval { expr, env } => {
            f(expr);
            f(env);
        }
        HirKind::Parameterize { bindings, body } => {
            for (p, v) in bindings {
                f(p);
                f(v);
            }
            f(body);
        }
        HirKind::DerefCell { cell } => f(cell),
        HirKind::SetCell { cell, value } => {
            f(cell);
            f(value);
        }
        HirKind::Nil
        | HirKind::EmptyList
        | HirKind::Bool(_)
        | HirKind::Int(_)
        | HirKind::Float(_)
        | HirKind::String(_)
        | HirKind::Keyword(_)
        | HirKind::Var(_)
        | HirKind::Quote(_)
        | HirKind::QuoteConst(_)
        | HirKind::Error => {}
    }
}
