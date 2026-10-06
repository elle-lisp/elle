// audited: 2026-10-05
//! The solver's vocabulary: interned signal variables, and the facts that the extractor states over them.
//!
//! docs/impl/solver.md

use std::collections::{BTreeSet, HashMap};

/// A file in the import graph, by index.
pub type FileId = u32;
/// A call site, by index.
pub type SiteId = u32;
/// An interned export key.
pub type KeyId = u32;
/// An interned signal variable.
pub type VarId = u32;

/// What a signal variable stands for. Every variable has a signal: the bits
/// it may raise, and the free variables whose signal it includes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum VarKey {
    /// A lambda node.
    Lam(FileId, u32),
    /// The code of a file outside every lambda.
    Top(FileId),
    /// A primitive, core or stdlib binding, by symbol id.
    Prim(u64),
    /// Parameter `idx` of the owner (a `Lam` or a `Prim`).
    Param(VarId, u32),
    /// Field `key` of a parameter.
    Field(VarId, KeyId),
    /// Export `key` of the module instance a call site created.
    Inst(SiteId, KeyId),
    /// The module instance a call site created, as a value.
    Obj(SiteId),
    /// The closure a literal-mask `squelch` or `attune` call returned.
    Sq(SiteId),
    /// A literal: data that runs no code, so calling it can only raise :error.
    Data,
    /// A value nothing is known about.
    Unknown,
}

/// The facts the extractor emits. Each field is one Datalog relation.
#[derive(Default)]
pub struct Facts {
    /// `raw(C, B)`: context C emits bit B itself.
    pub raw: BTreeSet<(VarId, u32)>,
    /// `bits(V, B)`: V raises bit B by itself. Primitives, literals and
    /// unknown values carry these.
    pub bits: BTreeSet<(VarId, u32)>,
    /// `dep(V, W)`: V includes the signal of free variable W. A primitive
    /// includes the parameters it propagates, and a free variable itself.
    pub dep: BTreeSet<(VarId, VarId)>,
    /// `use(C, S, G, O)`: C includes G's signal at site S, with O's
    /// parameters bound to S's arguments.
    pub uses: BTreeSet<(VarId, SiteId, VarId, VarId)>,
    /// `arg(S, I, A)`.
    pub arg: BTreeSet<(SiteId, u32, VarId)>,
    /// `noarg(S, I)`: S passes nothing at position I.
    pub noarg: BTreeSet<(SiteId, u32)>,
    /// `owner(W, P)`: free variable W belongs to P.
    pub owner: BTreeSet<(VarId, VarId)>,
    /// `pidx(W, O, I)`: W is parameter I of O.
    pub pidx: BTreeSet<(VarId, VarId, u32)>,
    /// `fidx(W, O, I, K)`: W is field K of parameter I of O.
    pub fidx: BTreeSet<(VarId, VarId, u32, KeyId)>,
    /// `fld(A, K, F)`: field K of value A has variable F.
    pub fld: BTreeSet<(VarId, KeyId, VarId)>,
    /// `isobj(A)`: A is a module instance whose fields are all listed.
    pub isobj: BTreeSet<VarId>,
    /// `hasfld(A, K)`.
    pub hasfld: BTreeSet<(VarId, KeyId)>,
    /// `opaque(A)`: A's fields are unknown.
    pub opaque: BTreeSet<VarId>,
    /// `muf(C, B)`: C muffles bit B.
    pub muf: BTreeSet<(VarId, u32)>,
    /// `ceiled(C)`: C declares a ceiling.
    pub ceiled: BTreeSet<VarId>,
    /// `ceil(C, B)`: C's ceiling allows bit B.
    pub ceil: BTreeSet<(VarId, u32)>,
    /// `sqof(Q, F)`: Q squelches F.
    pub sqof: BTreeSet<(VarId, VarId)>,
    /// `sqm(Q, B)`: Q's mask holds bit B.
    pub sqm: BTreeSet<(VarId, u32)>,
}

/// Interning tables, the facts, and what the report needs to name things.
#[derive(Default)]
pub struct Model {
    vars: HashMap<VarKey, VarId>,
    pub keys_by_id: Vec<VarKey>,
    keys: HashMap<String, KeyId>,
    pub key_names: Vec<String>,
    pub sites: u32,
    pub facts: Facts,
    /// Parameter count of each owner that has parameters.
    pub nparams: HashMap<VarId, u32>,
    /// The variable each call site's callee was resolved to, for the report.
    pub site_callee: HashMap<SiteId, VarId>,
    /// The argument count of each call that instantiates a module.
    pub site_nargs: HashMap<SiteId, u32>,
}

impl Model {
    /// Intern a variable. A free variable gets its self-dependency and its
    /// owner facts the first time it is seen.
    pub fn var(&mut self, key: VarKey) -> VarId {
        if let Some(&id) = self.vars.get(&key) {
            return id;
        }
        let id = self.keys_by_id.len() as VarId;
        self.keys_by_id.push(key.clone());
        self.vars.insert(key.clone(), id);
        match key {
            VarKey::Param(owner, idx) => {
                self.facts.dep.insert((id, id));
                self.facts.owner.insert((id, owner));
                self.facts.pidx.insert((id, owner, idx));
            }
            VarKey::Field(base, k) => {
                if let VarKey::Param(owner, idx) = self.keys_by_id[base as usize] {
                    self.facts.dep.insert((id, id));
                    self.facts.owner.insert((id, owner));
                    self.facts.fidx.insert((id, owner, idx, k));
                }
                self.facts.fld.insert((base, k, id));
            }
            VarKey::Data => {
                self.facts.bits.insert((id, 0));
                self.facts.opaque.insert(id);
            }
            VarKey::Unknown => {
                for b in all_bits() {
                    self.facts.bits.insert((id, b));
                }
                self.facts.opaque.insert(id);
            }
            _ => {}
        }
        id
    }

    /// Whether a variable is already interned.
    pub fn has(&self, key: &VarKey) -> bool {
        self.vars.contains_key(key)
    }

    /// The id of an interned variable.
    pub fn find(&self, key: &VarKey) -> Option<VarId> {
        self.vars.get(key).copied()
    }

    pub fn key(&mut self, name: &str) -> KeyId {
        if let Some(&k) = self.keys.get(name) {
            return k;
        }
        let k = self.key_names.len() as KeyId;
        self.key_names.push(name.to_string());
        self.keys.insert(name.to_string(), k);
        k
    }

    pub fn site(&mut self) -> SiteId {
        self.sites += 1;
        self.sites - 1
    }

    pub fn unknown(&mut self) -> VarId {
        self.var(VarKey::Unknown)
    }

    pub fn var_count(&self) -> usize {
        self.keys_by_id.len()
    }
}

/// The bit value that stands for every user-facing bit at once: what an
/// unknown callee may raise. One tuple instead of fifty keeps the model small.
pub const TOP: u32 = 64;

/// The bits an unknown callee may raise.
pub fn all_bits() -> impl Iterator<Item = u32> {
    std::iter::once(TOP)
}

/// The bit positions set in a signal-bit word, with `TOP` for a word that
/// holds every user-facing bit.
pub fn bit_positions(raw: u64) -> impl Iterator<Item = u32> {
    let cap = elle::signals::CAP_MASK.raw();
    let top = raw & cap == cap;
    let rest = if top { raw & !cap } else { raw };
    (0..64)
        .filter(move |b| rest & (1u64 << b) != 0)
        .chain(top.then_some(TOP))
}

/// A set of bit values back to a signal-bit word.
pub fn to_word(bits: impl Iterator<Item = u32>) -> u64 {
    bits.fold(0, |w, b| {
        if b == TOP {
            w | elle::signals::CAP_MASK.raw()
        } else {
            w | (1u64 << b)
        }
    })
}

/// The least model of the rules in docs/impl/solver.md: each variable's bits,
/// its free variables, and each ceiling a body raises past.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Solution {
    pub bits: BTreeSet<(VarId, u32)>,
    pub dep: BTreeSet<(VarId, VarId)>,
    pub viol: BTreeSet<(VarId, u32)>,
}

impl Model {
    /// State `use(ctx, site, callee, owner)`, and `noarg` for each parameter
    /// of the owner the call does not pass.
    pub fn use_fact(&mut self, ctx: VarId, site: SiteId, callee: VarId, owner: VarId, nargs: u32) {
        self.facts.uses.insert((ctx, site, callee, owner));
        let n = self.nparams.get(&owner).copied().unwrap_or(0);
        for i in nargs..n {
            self.facts.noarg.insert((site, i));
        }
    }
}
