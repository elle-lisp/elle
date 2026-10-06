// audited: 2026-10-05
//! Solve the rules in datalog.rs with the ascent crate, as a second in-process engine beside the worklist.
//!
//! docs/signals/inference.md

use crate::datalog::Solution;
use crate::link::Lowered;
use crate::model::{Model, TOP};
use ascent::ascent;

ascent! {
    struct Program;
    relation raw_in(u32, u32);
    relation bits_in(u32, u32);
    relation dep_in(u32, u32);
    relation owner(u32, u32);
    relation call(u32, u32, u32);
    relation sub(u32, u32, u32, u32);
    relation miss(u32, u32, u32);
    relation flow(u32, u32);
    relation muf(u32, u32);
    relation ceiled(u32);
    relation ceil(u32, u32);
    relation sqof(u32, u32);
    relation sqm(u32, u32);
    relation all(u32);
    relation raw(u32, u32);
    relation rdep(u32, u32);
    relation bits(u32, u32);
    relation dep(u32, u32);
    relation viol(u32, u32);

    raw(c, b) <-- raw_in(c, b);
    bits(v, b) <-- bits_in(v, b);
    dep(v, w) <-- dep_in(v, w);
    raw(c, b) <-- call(c, g, _o), bits(g, b);
    rdep(c, w) <-- call(c, g, o), dep(g, w), !owner(w, o);
    raw(c, b) <-- sub(c, g, w, a), dep(g, w), bits(a, b);
    rdep(c, x) <-- sub(c, g, w, a), dep(g, w), dep(a, x);
    raw(c, b) <-- miss(c, g, w), dep(g, w), all(b);
    raw(c, b) <-- flow(c, a), bits(a, b);
    rdep(c, x) <-- flow(c, a), dep(a, x);
    bits(c, b) <-- raw(c, b), !muf(c, b), !ceiled(c);
    bits(c, b) <-- ceil(c, b);
    dep(c, w) <-- rdep(c, w), !ceiled(c);
    bits(q, b) <-- sqof(q, f), bits(f, b), !sqm(q, b);
    bits(q, 0) <-- sqof(q, f), bits(f, b), sqm(q, b);
    dep(q, w) <-- sqof(q, f), dep(f, w);
    viol(c, b) <-- raw(c, b), ceiled(c), !ceil(c, b), !muf(c, b);
}

/// Compute the least model of the rules in datalog.rs.
pub fn solve(model: &Model, lowered: &Lowered) -> Solution {
    let f = &model.facts;
    let mut p = Program {
        raw_in: f.raw.iter().copied().collect(),
        bits_in: f.bits.iter().copied().collect(),
        dep_in: f.dep.iter().copied().collect(),
        owner: f.owner.iter().copied().collect(),
        call: lowered.call.iter().copied().collect(),
        sub: lowered.sub.iter().copied().collect(),
        miss: lowered.miss.iter().copied().collect(),
        flow: lowered.flow.iter().copied().collect(),
        muf: f.muf.iter().copied().collect(),
        ceiled: f.ceiled.iter().map(|&c| (c,)).collect(),
        ceil: f.ceil.iter().copied().collect(),
        sqof: f.sqof.iter().copied().collect(),
        sqm: f.sqm.iter().copied().collect(),
        all: vec![(TOP,)],
        ..Default::default()
    };
    p.run();
    Solution {
        bits: p.bits.into_iter().collect(),
        dep: p.dep.into_iter().collect(),
        viol: p.viol.into_iter().collect(),
    }
}
