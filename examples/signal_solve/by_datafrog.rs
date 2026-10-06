// audited: 2026-10-05
//! Solve the rules in datalog.rs with the datafrog crate, each join written by hand over sorted relations.
//!
//! docs/signals/inference.md

use crate::datalog::Solution;
use crate::link::Lowered;
use crate::model::{Model, TOP};
use datafrog::{Iteration, Relation, RelationLeaper, ValueFilter};
use std::collections::HashSet;

type Pair = (u32, u32);

/// Compute the least model of the rules in datalog.rs.
pub fn solve(model: &Model, lowered: &Lowered) -> Solution {
    let f = &model.facts;
    let call_by_g: Relation<(u32, Pair)> =
        lowered.call.iter().map(|&(c, g, o)| (g, (c, o))).collect();
    let sub_by_gw: Relation<(Pair, Pair)> = lowered
        .sub
        .iter()
        .map(|&(c, g, w, a)| ((g, w), (c, a)))
        .collect();
    let miss_by_gw: Relation<(Pair, u32)> =
        lowered.miss.iter().map(|&(c, g, w)| ((g, w), c)).collect();
    let flow_by_a: Relation<Pair> = lowered.flow.iter().map(|&(c, a)| (a, c)).collect();
    let sqof_by_f: Relation<Pair> = f.sqof.iter().map(|&(q, t)| (t, q)).collect();
    let muf: Relation<Pair> = f.muf.iter().copied().collect();
    let ceiled: Relation<u32> = f.ceiled.iter().copied().collect();
    let owner: HashSet<Pair> = f.owner.iter().copied().collect();
    let sqm: HashSet<Pair> = f.sqm.iter().copied().collect();

    let mut it = Iteration::new();
    // bits(v, b) and dep(v, w), keyed by their first column.
    let bits = it.variable::<Pair>("bits");
    let dep = it.variable::<Pair>("dep");
    // raw(c, b), keyed by the whole tuple so that muf can antijoin it.
    let raw = it.variable::<(Pair, ())>("raw");
    let unmuffled = it.variable::<Pair>("unmuffled");
    let rdep = it.variable::<Pair>("rdep");
    // act(a, c): a call from c binds a free variable of its callee to a.
    let act = it.variable::<Pair>("act");
    bits.extend(f.bits.iter().copied());
    bits.extend(f.ceil.iter().copied());
    dep.extend(f.dep.iter().copied());
    raw.extend(f.raw.iter().map(|&cb| (cb, ())));

    while it.changed() {
        raw.from_leapjoin(
            &bits,
            call_by_g.extend_with(|&(g, _)| g),
            |&(_, b), &(c, _)| ((c, b), ()),
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
        raw.from_join(&act, &bits, |_, &c, &b| ((c, b), ()));
        rdep.from_join(&act, &dep, |_, &c, &x| (c, x));
        raw.from_leapjoin(&dep, miss_by_gw.extend_with(|&gw| gw), |_, &c| {
            ((c, TOP), ())
        });
        raw.from_leapjoin(&bits, flow_by_a.extend_with(|&(a, _)| a), |&(_, b), &c| {
            ((c, b), ())
        });
        rdep.from_leapjoin(&dep, flow_by_a.extend_with(|&(a, _)| a), |&(_, x), &c| {
            (c, x)
        });
        unmuffled.from_antijoin(&raw, &muf, |&(c, b), _| (c, b));
        bits.from_antijoin(&unmuffled, &ceiled, |&c, &b| (c, b));
        dep.from_antijoin(&rdep, &ceiled, |&c, &w| (c, w));
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
    let ceil: HashSet<Pair> = f.ceil.iter().copied().collect();
    Solution {
        bits: bits.complete().iter().copied().collect(),
        dep: dep.complete().iter().copied().collect(),
        viol: raw
            .iter()
            .map(|&(cb, ())| cb)
            .filter(|&(c, b)| {
                f.ceiled.contains(&c) && !ceil.contains(&(c, b)) && !f.muf.contains(&(c, b))
            })
            .collect(),
    }
}
