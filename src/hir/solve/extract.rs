// audited: 2026-10-24
//! Walk one unit's analyzed HIR and emit the facts the solver reads.
//!
//! docs/impl/solver.md
//! docs/modules-proposal.md

mod call;
mod shape;
mod value;

pub use shape::ModuleShape;

use super::model::{bit_positions, FileId, Model, SiteId, VarId, VarKey};
use crate::hir::{Binding, BindingArena, Hir, HirId, HirKind, HirPattern, LambdaDecl, PatternKey};
use crate::signals::Signal;
use crate::value::fiber::SignalBits;
use crate::value::SymbolId;
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

/// One analyzed unit, as the extractor reads it.
pub struct Unit<'a> {
    pub id: FileId,
    pub hir: &'a Hir,
    pub arena: &'a BindingArena,
    pub decls: &'a HashMap<HirId, LambdaDecl>,
    /// What the code outside every lambda raises by itself.
    pub top_direct: SignalBits,
    /// The signal of each binding the compile started with.
    pub fixed: &'a HashMap<Binding, Signal>,
}

/// The site of a call node, and the context its signal is charged to.
#[derive(Debug, Clone, Copy)]
pub struct CallSite {
    pub site: SiteId,
    pub ctx: VarId,
}

pub struct Extractor<'a> {
    pub model: &'a mut Model,
    unit: &'a Unit<'a>,
    inits: HashMap<Binding, &'a Hir>,
    destructured: HashMap<Binding, (&'a Hir, String)>,
    param_of: HashMap<Binding, VarId>,
    memo: HashMap<Binding, AVal>,
    busy: HashSet<Binding>,
    sites: HashMap<HirId, SiteId>,
    /// Each call node the walk met, by its node.
    pub calls: HashMap<HirId, CallSite>,
    pub pending: Pending,
    /// The binding name of each lambda bound by a `def`, `let` or `letrec`.
    pub lambda_names: HashMap<HirId, SymbolId>,
}

impl<'a> Extractor<'a> {
    pub fn new(model: &'a mut Model, unit: &'a Unit<'a>) -> Self {
        let mut ex = Extractor {
            model,
            unit,
            inits: HashMap::new(),
            destructured: HashMap::new(),
            param_of: HashMap::new(),
            memo: HashMap::new(),
            busy: HashSet::new(),
            sites: HashMap::new(),
            calls: HashMap::new(),
            pending: Pending::default(),
            lambda_names: HashMap::new(),
        };
        ex.index(unit.hir);
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
            HirKind::Lambda(lambda) => {
                let lam = self.lam_var(hir.id);
                self.model.nparams.insert(lam, lambda.params.len() as u32);
                for (i, p) in lambda.params.iter().enumerate() {
                    let pv = self.model.var(VarKey::Param(lam, i as u32));
                    self.param_of.insert(*p, pv);
                }
                // A call of a bounded parameter raises what the bound admits,
                // whatever the caller passes.
                for pb in &lambda.param_bounds {
                    let pv = self.param_of[&pb.binding];
                    let bound = self.model.var(VarKey::Bound(pv));
                    for bit in bit_positions(pb.signal.bits.raw()) {
                        self.model.facts.bits.insert((bound, bit));
                    }
                }
                self.index(&lambda.body);
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
            _ => hir.for_each_child(|c| self.index(c)),
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

    /// The variable of a fixed binding, with its bits and the parameters it
    /// propagates.
    fn fixed(&mut self, b: Binding, sig: Signal) -> VarId {
        let key = VarKey::Fixed(self.unit.id, b);
        let fresh = !self.model.has(&key);
        let v = self.model.var(key);
        if fresh {
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

    /// Walk the whole unit, emitting facts, with `top` the variable of the
    /// code outside every lambda.
    pub fn walk_file(&mut self, top: VarId) {
        self.raises(top, self.unit.top_direct);
        self.walk(self.unit.hir, top);
    }

    /// State that `ctx` raises `bits` by itself.
    fn raises(&mut self, ctx: VarId, bits: SignalBits) {
        for bit in bit_positions(bits.raw()) {
            self.model.facts.raw.insert((ctx, bit));
        }
    }

    /// Walk the unit, emitting facts. `ctx` is the variable whose signal the
    /// code being walked contributes to. What a body raises by itself comes
    /// from the analyzer, so an `emit`, a `match` and every other form that
    /// may raise is walked only for the calls inside it.
    fn walk(&mut self, hir: &'a Hir, ctx: VarId) {
        match &hir.kind {
            HirKind::Lambda(lambda) => {
                let lam = self.lam_var(hir.id);
                let decl = *self
                    .unit
                    .decls
                    .get(&hir.id)
                    .expect("the analyzer declares every lambda");
                self.raises(lam, decl.direct);
                if let Some(c) = decl.ceiling {
                    self.model.facts.ceiled.insert(lam);
                    for bit in bit_positions(c.bits.raw()) {
                        self.model.facts.ceil.insert((lam, bit));
                    }
                }
                self.walk(&lambda.body, lam);
            }
            HirKind::Call { func, args, .. } => {
                self.call(hir, func, args, ctx);
                hir.for_each_child(|c| self.walk(c, ctx));
            }
            _ => hir.for_each_child(|c| self.walk(c, ctx)),
        }
    }
}
