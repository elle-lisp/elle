// audited: 2026-10-24
//! Solve the solver's rules again with the datafrog crate, as an independent check on the worklist's model.
//!
//! docs/impl/solver.md
//!
//! A relation here drops the site column, which the rules never join on.

use datafrog::{Iteration, Relation, RelationLeaper, ValueFilter};
use elle::hir::solve::{Lowered, Model, Solution, TOP};
use std::collections::HashSet;

type Pair = (u32, u32);

/// Compute the least model of the rules, each one a join over sorted relations.
pub fn solve(model: &Model, lowered: &Lowered) -> Solution {
    let f = &model.facts;
    let call_by_g: Relation<(u32, Pair)> = lowered
        .call
        .iter()
        .map(|&(c, _, g, o)| (g, (c, o)))
        .collect();
    let sub_by_gw: Relation<(Pair, Pair)> = lowered
        .sub
        .iter()
        .map(|&(c, _, g, w, a)| ((g, w), (c, a)))
        .collect();
    let miss_by_gw: Relation<(Pair, u32)> = lowered
        .miss
        .iter()
        .map(|&(c, _, g, w)| ((g, w), c))
        .collect();
    let flow_by_a: Relation<Pair> = lowered.flow.iter().map(|&(c, _, a)| (a, c)).collect();
    let sqof_by_f: Relation<Pair> = f.sqof.iter().map(|&(q, t)| (t, q)).collect();
    let owner: HashSet<Pair> = f.owner.iter().copied().collect();
    let sqm: HashSet<Pair> = f.sqm.iter().copied().collect();

    let mut it = Iteration::new();
    // bits(v, b) and dep(v, w), keyed by their first column.
    let bits = it.variable::<Pair>("bits");
    let dep = it.variable::<Pair>("dep");
    let raw = it.variable::<Pair>("raw");
    let rdep = it.variable::<Pair>("rdep");
    // act(a, c): a call from c binds a free variable of its callee to a.
    let act = it.variable::<Pair>("act");
    bits.extend(f.bits.iter().copied());
    dep.extend(f.dep.iter().copied());
    raw.extend(f.raw.iter().copied());

    while it.changed() {
        raw.from_leapjoin(
            &bits,
            call_by_g.extend_with(|&(g, _)| g),
            |&(_, b), &(c, _)| (c, b),
        );
        rdep.from_leapjoin(
            &dep,
            (
                call_by_g.extend_with(|&(g, _)| g),
                ValueFilter::from(|&(_, w): &Pair, &(_, o): &Pair| !owner.contains(&(w, o))),
            ),
            |&(_, w), &(c, _)| (c, w),
        );
        act.from_leapjoin(&dep, sub_by_gw.extend_with(|&gw| gw), |_, &(c, a)| (a, c));
        raw.from_join(&act, &bits, |_, &c, &b| (c, b));
        rdep.from_join(&act, &dep, |_, &c, &x| (c, x));
        raw.from_leapjoin(&dep, miss_by_gw.extend_with(|&gw| gw), |_, &c| (c, TOP));
        raw.from_leapjoin(&bits, flow_by_a.extend_with(|&(a, _)| a), |&(_, b), &c| {
            (c, b)
        });
        rdep.from_leapjoin(&dep, flow_by_a.extend_with(|&(a, _)| a), |&(_, x), &c| {
            (c, x)
        });
        // A ceiling checks the body and does not replace it.
        bits.from_map(&raw, |&cb| cb);
        dep.from_map(&rdep, |&cw| cw);
        bits.from_leapjoin(
            &bits,
            (
                sqof_by_f.extend_with(|&(t, _)| t),
                ValueFilter::from(|&(_, b): &Pair, &q: &u32| !sqm.contains(&(q, b))),
            ),
            |&(_, b), &q| (q, b),
        );
        bits.from_leapjoin(
            &bits,
            (
                sqof_by_f.extend_with(|&(t, _)| t),
                ValueFilter::from(|&(_, b): &Pair, &q: &u32| sqm.contains(&(q, b))),
            ),
            |_, &q| (q, 0),
        );
        dep.from_leapjoin(&dep, sqof_by_f.extend_with(|&(t, _)| t), |&(_, w), &q| {
            (q, w)
        });
    }

    let raw = raw.complete();
    let rdep = rdep.complete();
    let ceil: HashSet<Pair> = f.ceil.iter().copied().collect();
    Solution {
        bits: bits.complete().iter().copied().collect(),
        dep: dep.complete().iter().copied().collect(),
        viol: raw
            .iter()
            .copied()
            .filter(|&(c, b)| f.ceiled.contains(&c) && !ceil.contains(&(c, b)))
            .collect(),
        vdep: rdep
            .iter()
            .copied()
            .filter(|&(c, _)| f.ceiled.contains(&c))
            .collect(),
    }
}
