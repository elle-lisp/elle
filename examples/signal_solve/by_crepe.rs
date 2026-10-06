// audited: 2026-10-05
//! Solve the rules in datalog.rs with the crepe crate, as a second in-process engine beside the worklist.
//!
//! docs/signals/inference.md

use crate::datalog::Solution;
use crate::link::Lowered;
use crate::model::{Model, TOP};
use crepe::crepe;

crepe! {
    @input struct RawIn(u32, u32);
    @input struct BitsIn(u32, u32);
    @input struct DepIn(u32, u32);
    @input struct Owner(u32, u32);
    @input struct Call(u32, u32, u32);
    @input struct Sub(u32, u32, u32, u32);
    @input struct Miss(u32, u32, u32);
    @input struct Flow(u32, u32);
    @input struct Muf(u32, u32);
    @input struct Ceiled(u32);
    @input struct Ceil(u32, u32);
    @input struct Sqof(u32, u32);
    @input struct Sqm(u32, u32);
    @input struct All(u32);
    struct Raw(u32, u32);
    struct Rdep(u32, u32);
    @output struct Bits(u32, u32);
    @output struct Dep(u32, u32);
    @output struct Viol(u32, u32);

    Raw(c, b) <- RawIn(c, b);
    Bits(v, b) <- BitsIn(v, b);
    Dep(v, w) <- DepIn(v, w);
    Raw(c, b) <- Call(c, g, _), Bits(g, b);
    Rdep(c, w) <- Call(c, g, o), Dep(g, w), !Owner(w, o);
    Raw(c, b) <- Sub(c, g, w, a), Dep(g, w), Bits(a, b);
    Rdep(c, x) <- Sub(c, g, w, a), Dep(g, w), Dep(a, x);
    Raw(c, b) <- Miss(c, g, w), Dep(g, w), All(b);
    Raw(c, b) <- Flow(c, a), Bits(a, b);
    Rdep(c, x) <- Flow(c, a), Dep(a, x);
    Bits(c, b) <- Raw(c, b), !Muf(c, b), !Ceiled(c);
    Bits(c, b) <- Ceil(c, b);
    Dep(c, w) <- Rdep(c, w), !Ceiled(c);
    Bits(q, b) <- Sqof(q, f), Bits(f, b), !Sqm(q, b);
    Bits(q, 0) <- Sqof(q, f), Bits(f, b), Sqm(q, b);
    Dep(q, w) <- Sqof(q, f), Dep(f, w);
    Viol(c, b) <- Raw(c, b), Ceiled(c), !Ceil(c, b), !Muf(c, b);
}

/// Compute the least model of the rules in datalog.rs.
pub fn solve(model: &Model, lowered: &Lowered) -> Solution {
    let f = &model.facts;
    let mut rt = Crepe::new();
    rt.extend(f.raw.iter().map(|&(c, b)| RawIn(c, b)));
    rt.extend(f.bits.iter().map(|&(v, b)| BitsIn(v, b)));
    rt.extend(f.dep.iter().map(|&(v, w)| DepIn(v, w)));
    rt.extend(f.owner.iter().map(|&(w, o)| Owner(w, o)));
    rt.extend(lowered.call.iter().map(|&(c, g, o)| Call(c, g, o)));
    rt.extend(lowered.sub.iter().map(|&(c, g, w, a)| Sub(c, g, w, a)));
    rt.extend(lowered.miss.iter().map(|&(c, g, w)| Miss(c, g, w)));
    rt.extend(lowered.flow.iter().map(|&(c, a)| Flow(c, a)));
    rt.extend(f.muf.iter().map(|&(c, b)| Muf(c, b)));
    rt.extend(f.ceiled.iter().map(|&c| Ceiled(c)));
    rt.extend(f.ceil.iter().map(|&(c, b)| Ceil(c, b)));
    rt.extend(f.sqof.iter().map(|&(q, t)| Sqof(q, t)));
    rt.extend(f.sqm.iter().map(|&(q, b)| Sqm(q, b)));
    rt.extend([All(TOP)]);
    let (bits, dep, viol) = rt.run();
    Solution {
        bits: bits.into_iter().map(|Bits(v, b)| (v, b)).collect(),
        dep: dep.into_iter().map(|Dep(v, w)| (v, w)).collect(),
        viol: viol.into_iter().map(|Viol(c, b)| (c, b)).collect(),
    }
}
