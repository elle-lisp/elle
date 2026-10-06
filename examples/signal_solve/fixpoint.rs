// audited: 2026-10-05
//! The solver: compute the least model of the solver's rules with a worklist over the variables.
//!
//! docs/impl/solver.md

use crate::link::Lowered;
use crate::model::{Model, Solution, VarId, TOP};
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

/// A set of bit values, `TOP` included.
type Bits = u128;

fn word(bits: &BTreeSet<(VarId, u32)>) -> HashMap<VarId, Bits> {
    let mut m: HashMap<VarId, Bits> = HashMap::new();
    for &(v, b) in bits {
        *m.entry(v).or_default() |= 1u128 << b;
    }
    m
}

/// The input facts, indexed by the context that reads them.
struct Tables {
    raw: HashMap<VarId, Bits>,
    owner: HashSet<(VarId, VarId)>,
    calls: HashMap<VarId, Vec<(VarId, VarId)>>,
    subs: HashMap<VarId, Vec<(VarId, VarId, VarId)>>,
    misses: HashMap<VarId, Vec<(VarId, VarId)>>,
    flows: HashMap<VarId, Vec<VarId>>,
}

/// The current model: each variable's bits and free variables.
struct State {
    bits: HashMap<VarId, Bits>,
    dep: HashMap<VarId, BTreeSet<VarId>>,
}

impl State {
    fn bits(&self, v: VarId) -> Bits {
        self.bits.get(&v).copied().unwrap_or(0)
    }

    fn has_dep(&self, v: VarId, w: VarId) -> bool {
        self.dep.get(&v).is_some_and(|d| d.contains(&w))
    }

    fn deps(&self, v: VarId) -> impl Iterator<Item = VarId> + '_ {
        self.dep.get(&v).into_iter().flatten().copied()
    }
}

/// `raw` and `rdep` of one context: what its body raises and the free
/// variables it reads, before a ceiling or a muffle applies.
fn gather(c: VarId, t: &Tables, s: &State) -> (Bits, BTreeSet<VarId>) {
    let mut raw: Bits = t.raw.get(&c).copied().unwrap_or(0);
    let mut rdep: BTreeSet<VarId> = BTreeSet::new();
    for &(g, o) in t.calls.get(&c).into_iter().flatten() {
        raw |= s.bits(g);
        rdep.extend(s.deps(g).filter(|&w| !t.owner.contains(&(w, o))));
    }
    for &(g, w, a) in t.subs.get(&c).into_iter().flatten() {
        if s.has_dep(g, w) {
            raw |= s.bits(a);
            rdep.extend(s.deps(a));
        }
    }
    for &(g, w) in t.misses.get(&c).into_iter().flatten() {
        if s.has_dep(g, w) {
            raw |= 1u128 << TOP;
        }
    }
    for &a in t.flows.get(&c).into_iter().flatten() {
        raw |= s.bits(a);
        rdep.extend(s.deps(a));
    }
    (raw, rdep)
}

/// Compute the least model of the rules.
pub fn solve(model: &Model, lowered: &Lowered) -> Solution {
    let f = &model.facts;
    let base_bits = word(&f.bits);
    let muf = word(&f.muf);
    let ceil = word(&f.ceil);
    let sqm = word(&f.sqm);
    let mut base_dep: HashMap<VarId, BTreeSet<VarId>> = HashMap::new();
    for &(v, w) in &f.dep {
        base_dep.entry(v).or_default().insert(w);
    }

    // What each context reads, and who reads each variable.
    let mut t = Tables {
        raw: word(&f.raw),
        owner: f.owner.iter().copied().collect(),
        calls: HashMap::new(),
        subs: HashMap::new(),
        misses: HashMap::new(),
        flows: HashMap::new(),
    };
    let mut readers: HashMap<VarId, Vec<VarId>> = HashMap::new();
    for &(c, g, o) in &lowered.call {
        t.calls.entry(c).or_default().push((g, o));
        readers.entry(g).or_default().push(c);
    }
    for &(c, g, w, a) in &lowered.sub {
        t.subs.entry(c).or_default().push((g, w, a));
        readers.entry(g).or_default().push(c);
        readers.entry(a).or_default().push(c);
    }
    for &(c, g, w) in &lowered.miss {
        t.misses.entry(c).or_default().push((g, w));
        readers.entry(g).or_default().push(c);
    }
    for &(c, a) in &lowered.flow {
        t.flows.entry(c).or_default().push(a);
        readers.entry(a).or_default().push(c);
    }
    let mut squelch: HashMap<VarId, VarId> = HashMap::new();
    for &(q, target) in &f.sqof {
        squelch.insert(q, target);
        readers.entry(target).or_default().push(q);
    }

    let mut s = State {
        bits: base_bits.clone(),
        dep: base_dep.clone(),
    };
    let mut queue: VecDeque<VarId> = (0..model.var_count() as VarId).collect();
    let mut queued: HashSet<VarId> = queue.iter().copied().collect();
    while let Some(c) = queue.pop_front() {
        queued.remove(&c);
        let (raw, rdep) = gather(c, &t, &s);
        let mut new_bits = base_bits.get(&c).copied().unwrap_or(0);
        let mut new_dep = base_dep.get(&c).cloned().unwrap_or_default();
        if f.ceiled.contains(&c) {
            new_bits |= ceil.get(&c).copied().unwrap_or(0);
        } else {
            new_bits |= raw & !muf.get(&c).copied().unwrap_or(0);
            new_dep.extend(rdep);
        }
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

    let mut out = Solution::default();
    for (&v, &b) in &s.bits {
        out.bits.extend(bit_values(b).map(|i| (v, i)));
    }
    for (&v, ws) in &s.dep {
        out.dep.extend(ws.iter().map(|&w| (v, w)));
    }
    // A ceiling hides what its body raises from every reader, so the bits past
    // it are read off the settled model, once.
    for &c in &f.ceiled {
        let (raw, _) = gather(c, &t, &s);
        let past = raw & !ceil.get(&c).copied().unwrap_or(0) & !muf.get(&c).copied().unwrap_or(0);
        out.viol.extend(bit_values(past).map(|i| (c, i)));
    }
    out
}

/// The bit values set in a `Bits` word.
fn bit_values(b: Bits) -> impl Iterator<Item = u32> {
    (0..=TOP).filter(move |&i| b & (1u128 << i) != 0)
}
