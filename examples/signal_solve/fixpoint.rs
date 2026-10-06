// audited: 2026-10-05
//! Solve the rules in datalog.rs with a worklist in Rust, as a check on z3's model.
//!
//! docs/signals/inference.md

use crate::datalog::Solution;
use crate::link::Lowered;
use crate::model::{Model, VarId, TOP};
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

/// Compute the least model of the rules in datalog.rs.
pub fn solve(model: &Model, lowered: &Lowered) -> Solution {
    let f = &model.facts;
    let raw_facts = word(&f.raw);
    let base_bits = word(&f.bits);
    let muf = word(&f.muf);
    let ceil = word(&f.ceil);
    let sqm = word(&f.sqm);
    let mut base_dep: HashMap<VarId, BTreeSet<VarId>> = HashMap::new();
    for &(v, w) in &f.dep {
        base_dep.entry(v).or_default().insert(w);
    }
    let owner: HashSet<(VarId, VarId)> = f.owner.iter().copied().collect();

    // What each context reads, and who reads each variable.
    let mut calls: HashMap<VarId, Vec<(VarId, VarId)>> = HashMap::new();
    let mut subs: HashMap<VarId, Vec<(VarId, VarId, VarId)>> = HashMap::new();
    let mut misses: HashMap<VarId, Vec<(VarId, VarId)>> = HashMap::new();
    let mut readers: HashMap<VarId, Vec<VarId>> = HashMap::new();
    for &(c, g, o) in &lowered.call {
        calls.entry(c).or_default().push((g, o));
        readers.entry(g).or_default().push(c);
    }
    for &(c, g, w, a) in &lowered.sub {
        subs.entry(c).or_default().push((g, w, a));
        readers.entry(g).or_default().push(c);
        readers.entry(a).or_default().push(c);
    }
    for &(c, g, w) in &lowered.miss {
        misses.entry(c).or_default().push((g, w));
        readers.entry(g).or_default().push(c);
    }
    let mut flows: HashMap<VarId, Vec<VarId>> = HashMap::new();
    for &(c, a) in &lowered.flow {
        flows.entry(c).or_default().push(a);
        readers.entry(a).or_default().push(c);
    }
    let mut squelch: HashMap<VarId, VarId> = HashMap::new();
    for &(q, t) in &f.sqof {
        squelch.insert(q, t);
        readers.entry(t).or_default().push(q);
    }

    let mut bits: HashMap<VarId, Bits> = base_bits.clone();
    let mut dep: HashMap<VarId, BTreeSet<VarId>> = base_dep.clone();
    let mut queue: VecDeque<VarId> = (0..model.var_count() as VarId).collect();
    let mut queued: HashSet<VarId> = queue.iter().copied().collect();
    let empty = BTreeSet::new();
    while let Some(c) = queue.pop_front() {
        queued.remove(&c);
        let mut raw: Bits = raw_facts.get(&c).copied().unwrap_or(0);
        let mut rdep: BTreeSet<VarId> = BTreeSet::new();
        for &(g, o) in calls.get(&c).map(|v| v.as_slice()).unwrap_or(&[]) {
            raw |= bits.get(&g).copied().unwrap_or(0);
            for &w in dep.get(&g).unwrap_or(&empty) {
                if !owner.contains(&(w, o)) {
                    rdep.insert(w);
                }
            }
        }
        for &(g, w, a) in subs.get(&c).map(|v| v.as_slice()).unwrap_or(&[]) {
            if dep.get(&g).is_some_and(|d| d.contains(&w)) {
                raw |= bits.get(&a).copied().unwrap_or(0);
                rdep.extend(dep.get(&a).unwrap_or(&empty).iter().copied());
            }
        }
        for &(g, w) in misses.get(&c).map(|v| v.as_slice()).unwrap_or(&[]) {
            if dep.get(&g).is_some_and(|d| d.contains(&w)) {
                raw |= 1u128 << TOP;
            }
        }
        for &a in flows.get(&c).map(|v| v.as_slice()).unwrap_or(&[]) {
            raw |= bits.get(&a).copied().unwrap_or(0);
            rdep.extend(dep.get(&a).unwrap_or(&empty).iter().copied());
        }
        let ceiled = f.ceiled.contains(&c);
        let mut new_bits = base_bits.get(&c).copied().unwrap_or(0);
        let mut new_dep = base_dep.get(&c).cloned().unwrap_or_default();
        if ceiled {
            new_bits |= ceil.get(&c).copied().unwrap_or(0);
        } else {
            new_bits |= raw & !muf.get(&c).copied().unwrap_or(0);
            new_dep.extend(rdep);
        }
        if let Some(&t) = squelch.get(&c) {
            let tb = bits.get(&t).copied().unwrap_or(0);
            let mask = sqm.get(&c).copied().unwrap_or(0);
            new_bits |= tb & !mask;
            if tb & mask != 0 {
                new_bits |= 1;
            }
            new_dep.extend(dep.get(&t).unwrap_or(&empty).iter().copied());
        }
        let changed = bits.get(&c).copied().unwrap_or(0) != new_bits
            || dep.get(&c).unwrap_or(&empty) != &new_dep;
        if changed {
            bits.insert(c, new_bits);
            dep.insert(c, new_dep);
            for &r in readers.get(&c).map(|v| v.as_slice()).unwrap_or(&[]) {
                if queued.insert(r) {
                    queue.push_back(r);
                }
            }
        }
    }

    let mut out = Solution::default();
    for (&v, &b) in &bits {
        for i in 0..=TOP {
            if b & (1u128 << i) != 0 {
                out.bits.insert((v, i));
            }
        }
    }
    for (&v, ws) in &dep {
        for &w in ws {
            out.dep.insert((v, w));
        }
    }
    out
}
