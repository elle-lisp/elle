// audited: 2026-10-24
//! How the solver's answer for each lambda relates to the analyzer's, tallied and printed.
//!
//! docs/impl/solver.md

use super::{fmt_bits, Context, Lambda};
use elle::hir::solve::{VarId, VarKey};
use std::collections::{BTreeMap, HashMap};

/// How the solver's answer for one lambda relates to the analyzer's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Verdict {
    Same,
    /// Fewer bits or fewer dependencies: the analyzer fell back to a
    /// conservative answer the solver did not need.
    Tighter,
    /// Bits or dependencies the analyzer lacks.
    Looser,
    Incomparable,
}

/// Compare each lambda's solved signal with the analyzer's. Print the tally,
/// then each row that differs, or every row under `all`.
pub(super) fn compare(
    ctx: &Context,
    lambdas: &[Lambda],
    bits_of: &HashMap<VarId, u64>,
    deps_of: &HashMap<VarId, Vec<VarId>>,
    lines: &HashMap<(u32, u32), usize>,
    all: bool,
) {
    let mut tally: BTreeMap<Verdict, usize> = BTreeMap::new();
    let mut rows: Vec<(Verdict, String)> = Vec::new();
    for l in lambdas {
        let Some(v) = ctx.model.find(&VarKey::Lam(l.unit, l.id)) else {
            continue;
        };
        let s_bits = bits_of.get(&v).copied().unwrap_or(0);
        let mut s_prop = 0u32;
        let mut free = Vec::new();
        for &w in deps_of.get(&v).map(|d| d.as_slice()).unwrap_or(&[]) {
            match ctx.model.keys_by_id[w as usize] {
                VarKey::Param(o, i) if o == v => s_prop |= 1 << i,
                _ => free.push(ctx.describe(w, lines)),
            }
        }
        free.sort();
        // VM-internal bits name no transfer a program can catch, so neither
        // side is compared on them.
        let cap = elle::signals::CAP_MASK.raw();
        let s_bits = s_bits & cap;
        let a_bits = l.inferred.bits.raw() & cap;
        let a_prop = l.inferred.propagates;
        // An analyzer answer holding every user-facing bit is "unknown": it
        // already covers whatever any parameter may raise.
        let a_unknown = a_bits == cap;
        let same = a_bits == s_bits && a_prop == s_prop && free.is_empty();
        let verdict = if same || (a_unknown && s_bits == cap) {
            Verdict::Same
        } else if a_unknown || (s_bits & !a_bits == 0 && s_prop & !a_prop == 0) {
            Verdict::Tighter
        } else if a_bits & !s_bits == 0 && a_prop & !s_prop == 0 {
            Verdict::Looser
        } else {
            Verdict::Incomparable
        };
        *tally.entry(verdict).or_default() += 1;
        if verdict != Verdict::Same || all {
            let free = if free.is_empty() {
                String::new()
            } else {
                format!(" + {{{}}}", free.join(", "))
            };
            rows.push((
                verdict,
                format!(
                    "{:?} {}\n    analyzer {} prop {:b}\n    solver   {} prop {:b}{}",
                    verdict,
                    ctx.describe(v, lines),
                    fmt_bits(a_bits),
                    a_prop,
                    fmt_bits(s_bits),
                    s_prop,
                    free
                ),
            ));
        }
    }
    println!("lambdas: {:?}", tally);
    rows.sort();
    let mut shown: BTreeMap<Verdict, usize> = BTreeMap::new();
    for (v, row) in &rows {
        let n = shown.entry(*v).or_default();
        *n += 1;
        if all || *v != Verdict::Tighter || *n <= 15 {
            println!("{row}");
        }
    }
}
