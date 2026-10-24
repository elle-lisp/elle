// audited: 2026-10-24
//! The solver: compute the least model of the solver's rules with a worklist over the variables.
//!
//! docs/impl/solver.md

use super::link::Lowered;
use super::model::{Model, SiteId, Solution, VarId, TOP};
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

/// A set of bit values, `TOP` included.
pub type Bits = u128;

fn word(bits: &BTreeSet<(VarId, u32)>) -> HashMap<VarId, Bits> {
    let mut m: HashMap<VarId, Bits> = HashMap::new();
    for &(v, b) in bits {
        *m.entry(v).or_default() |= 1u128 << b;
    }
    m
}

/// The input facts, indexed by the context that reads them. Each entry keeps
/// the site it came from.
#[derive(Default)]
struct Tables {
    raw: HashMap<VarId, Bits>,
    owner: HashSet<(VarId, VarId)>,
    calls: HashMap<VarId, Vec<(SiteId, VarId, VarId)>>,
    subs: HashMap<VarId, Vec<(SiteId, VarId, VarId, VarId)>>,
    misses: HashMap<VarId, Vec<(SiteId, VarId, VarId)>>,
    flows: HashMap<VarId, Vec<(SiteId, VarId)>>,
}

/// The settled model: each variable's bits and free variables, and the
/// tables that answer a context or one call in it.
pub struct Solved {
    bits: HashMap<VarId, Bits>,
    dep: HashMap<VarId, BTreeSet<VarId>>,
    tables: Tables,
}

impl Solved {
    /// The bit values `v` raises.
    pub fn bits(&self, v: VarId) -> Bits {
        self.bits.get(&v).copied().unwrap_or(0)
    }

    fn has_dep(&self, v: VarId, w: VarId) -> bool {
        self.dep.get(&v).is_some_and(|d| d.contains(&w))
    }

    /// The free variables whose signal `v` includes.
    pub fn deps(&self, v: VarId) -> impl Iterator<Item = VarId> + '_ {
        self.dep.get(&v).into_iter().flatten().copied()
    }

    /// `raw` and `rdep` of context `c`: what its body raises and the free
    /// variables it reads. Given a site, only the tuples that call adds.
    pub fn gather(&self, c: VarId, site: Option<SiteId>) -> (Bits, BTreeSet<VarId>) {
        let t = &self.tables;
        let at = |s: SiteId| site.is_none_or(|want| want == s);
        let mut raw: Bits = if site.is_none() {
            t.raw.get(&c).copied().unwrap_or(0)
        } else {
            0
        };
        let mut rdep: BTreeSet<VarId> = BTreeSet::new();
        for &(s, g, o) in t.calls.get(&c).into_iter().flatten() {
            if at(s) {
                raw |= self.bits(g);
                rdep.extend(self.deps(g).filter(|&w| !t.owner.contains(&(w, o))));
            }
        }
        for &(s, g, w, a) in t.subs.get(&c).into_iter().flatten() {
            if at(s) && self.has_dep(g, w) {
                raw |= self.bits(a);
                rdep.extend(self.deps(a));
            }
        }
        for &(s, g, w) in t.misses.get(&c).into_iter().flatten() {
            if at(s) && self.has_dep(g, w) {
                raw |= 1u128 << TOP;
            }
        }
        for &(s, a) in t.flows.get(&c).into_iter().flatten() {
            if at(s) {
                raw |= self.bits(a);
                rdep.extend(self.deps(a));
            }
        }
        (raw, rdep)
    }

    /// The model as relations, with what each ceiled body raises past its
    /// ceiling and the free variables it depends on.
    pub fn solution(&self, model: &Model) -> Solution {
        let f = &model.facts;
        let ceil = word(&f.ceil);
        let mut out = Solution::default();
        for (&v, &b) in &self.bits {
            out.bits.extend(bit_values(b).map(|i| (v, i)));
        }
        for (&v, ws) in &self.dep {
            out.dep.extend(ws.iter().map(|&w| (v, w)));
        }
        for &c in &f.ceiled {
            let (raw, rdep) = self.gather(c, None);
            let past = raw & !ceil.get(&c).copied().unwrap_or(0);
            out.viol.extend(bit_values(past).map(|i| (c, i)));
            out.vdep.extend(rdep.into_iter().map(|w| (c, w)));
        }
        out
    }
}

/// Compute the least model of the rules.
pub fn solve(model: &Model, lowered: &Lowered) -> Solved {
    let f = &model.facts;
    let base_bits = word(&f.bits);
    let sqm = word(&f.sqm);
    let mut base_dep: HashMap<VarId, BTreeSet<VarId>> = HashMap::new();
    for &(v, w) in &f.dep {
        base_dep.entry(v).or_default().insert(w);
    }

    // What each context reads, and who reads each variable.
    let mut t = Tables {
        raw: word(&f.raw),
        owner: f.owner.iter().copied().collect(),
        ..Tables::default()
    };
    let mut readers: HashMap<VarId, Vec<VarId>> = HashMap::new();
    for &(c, s, g, o) in &lowered.call {
        t.calls.entry(c).or_default().push((s, g, o));
        readers.entry(g).or_default().push(c);
    }
    for &(c, s, g, w, a) in &lowered.sub {
        t.subs.entry(c).or_default().push((s, g, w, a));
        readers.entry(g).or_default().push(c);
        readers.entry(a).or_default().push(c);
    }
    for &(c, s, g, w) in &lowered.miss {
        t.misses.entry(c).or_default().push((s, g, w));
        readers.entry(g).or_default().push(c);
    }
    for &(c, s, a) in &lowered.flow {
        t.flows.entry(c).or_default().push((s, a));
        readers.entry(a).or_default().push(c);
    }
    let mut squelch: HashMap<VarId, VarId> = HashMap::new();
    for &(q, target) in &f.sqof {
        squelch.insert(q, target);
        readers.entry(target).or_default().push(q);
    }

    let mut s = Solved {
        bits: base_bits.clone(),
        dep: base_dep.clone(),
        tables: t,
    };
    let mut queue: VecDeque<VarId> = (0..model.var_count() as VarId).collect();
    let mut queued: HashSet<VarId> = queue.iter().copied().collect();
    while let Some(c) = queue.pop_front() {
        queued.remove(&c);
        // A ceiling checks the body and does not replace it, so a ceiled
        // context's readers see what its body raises.
        let (raw, rdep) = s.gather(c, None);
        let mut new_bits = base_bits.get(&c).copied().unwrap_or(0) | raw;
        let mut new_dep = base_dep.get(&c).cloned().unwrap_or_default();
        new_dep.extend(rdep);
        if let Some(&target) = squelch.get(&c) {
            let tb = s.bits(target);
            let mask = sqm.get(&c).copied().unwrap_or(0);
            new_bits |= tb & !mask;
            if tb & mask != 0 {
                new_bits |= 1;
            }
            new_dep.extend(s.deps(target));
        }
        let changed = s.bits(c) != new_bits || s.dep.get(&c) != Some(&new_dep);
        if changed {
            s.bits.insert(c, new_bits);
            s.dep.insert(c, new_dep);
            for &r in readers.get(&c).into_iter().flatten() {
                if queued.insert(r) {
                    queue.push_back(r);
                }
            }
        }
    }
    s
}

/// The bit values set in a `Bits` word.
pub fn bit_values(b: Bits) -> impl Iterator<Item = u32> {
    (0..=TOP).filter(move |&i| b & (1u128 << i) != 0)
}
