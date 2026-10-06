// audited: 2026-10-05
//! Compare the solver's least model with the analyzer's inferred signals, and print what the run found.
//!
//! docs/impl/solver.md

use crate::extract::{ModuleShape, Unit};
use crate::model::{Model, Solution, VarId, VarKey};
use crate::visit::each_child;
use elle::hir::{Hir, HirKind};
use elle::signals::Signal;
use elle::value::fiber::SignalBits;
use elle::value::SymbolId;
use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

pub struct Context<'a> {
    pub model: &'a Model,
    pub solution: &'a Solution,
    pub units: &'a [Unit],
    pub names: &'a HashMap<(u32, u32), SymbolId>,
    pub symbols: &'a elle::SymbolTable,
    pub shapes: &'a HashMap<String, ModuleShape>,
}

pub struct Run<'a> {
    pub all: bool,
    pub failed: &'a [(String, String)],
    pub analyze_time: Duration,
    pub extract_time: Duration,
    pub solve_time: Duration,
    pub check_time: Duration,
    pub expect: bool,
}

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

pub fn fmt_bits(bits: u64) -> String {
    elle::signals::registry::format_bits(SignalBits::new(bits))
}

/// A lambda node of an analyzed file, and what the analyzer inferred for it.
pub struct Lambda {
    pub unit: u32,
    pub id: u32,
    line: usize,
    inferred: Signal,
}

fn lambdas(hir: &Hir, unit: u32, out: &mut Vec<Lambda>) {
    if let HirKind::Lambda {
        inferred_signals, ..
    } = &hir.kind
    {
        out.push(Lambda {
            unit,
            id: hir.id.0,
            line: hir.span.line as usize,
            inferred: *inferred_signals,
        });
    }
    each_child(hir, &mut |c| lambdas(c, unit, out));
}

impl<'a> Context<'a> {
    /// The last two components of a file's path.
    pub fn file(&self, unit: u32) -> String {
        let p = &self.units[unit as usize].path;
        p.rsplit('/')
            .take(2)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("/")
    }

    /// A variable, named for a reader.
    pub fn describe_var(&self, v: VarId, all: &[Lambda]) -> String {
        let lines: HashMap<(u32, u32), usize> =
            all.iter().map(|l| ((l.unit, l.id), l.line)).collect();
        self.describe(v, &lines)
    }

    fn describe(&self, v: VarId, lines: &HashMap<(u32, u32), usize>) -> String {
        match &self.model.keys_by_id[v as usize] {
            VarKey::Lam(f, id) => {
                let name = self
                    .names
                    .get(&(*f, *id))
                    .and_then(|s| self.symbols.name(*s))
                    .unwrap_or("fn");
                let line = lines.get(&(*f, *id)).copied().unwrap_or(0);
                format!("{}@{}:{}", name, self.file(*f), line)
            }
            VarKey::Top(f) => format!("top of {}", self.file(*f)),
            VarKey::Prim(s) => self
                .symbols
                .name(SymbolId(*s))
                .unwrap_or("<prim>")
                .to_string(),
            VarKey::Param(o, i) => format!("param {} of {}", i, self.describe(*o, lines)),
            VarKey::Field(p, k) => {
                format!(
                    "({}):{}",
                    self.describe(*p, lines),
                    self.model.key_names[*k as usize]
                )
            }
            VarKey::Inst(s, k) => format!("instance#{}:{}", s, self.model.key_names[*k as usize]),
            VarKey::Obj(s) => format!("instance#{}", s),
            VarKey::Sq(s) => format!("squelch#{}", s),
            VarKey::Data => "data".to_string(),
            VarKey::Unknown => "unknown".to_string(),
        }
    }
}

/// Name every tuple one model holds and the other lacks, for a run whose two
/// engines disagree. Returns the number of tuples that differ.
pub fn disagreement(ctx: &Context, check: &Solution) -> usize {
    let s = ctx.solution;
    let mut n = 0;
    for (name, worklist, datafrog) in [
        ("bits", &s.bits, &check.bits),
        ("dep", &s.dep, &check.dep),
        ("viol", &s.viol, &check.viol),
    ] {
        for (only, a, b) in [
            ("the worklist", worklist, datafrog),
            ("datafrog", datafrog, worklist),
        ] {
            for &(v, x) in a.difference(b) {
                n += 1;
                if n <= 20 {
                    println!("  {name}({}, {x}) only in {only}", ctx.describe_var(v, &[]));
                }
            }
        }
    }
    n
}

pub fn print(ctx: &Context, run: &Run) -> usize {
    let mut bits_of: HashMap<VarId, u64> = HashMap::new();
    for &(v, b) in &ctx.solution.bits {
        *bits_of.entry(v).or_default() |= crate::model::to_word(std::iter::once(b));
    }
    let mut deps_of: HashMap<VarId, Vec<VarId>> = HashMap::new();
    for &(v, w) in &ctx.solution.dep {
        deps_of.entry(v).or_default().push(w);
    }

    let f = &ctx.model.facts;
    let nfacts = f.raw.len()
        + f.bits.len()
        + f.dep.len()
        + f.uses.len()
        + f.arg.len()
        + f.noarg.len()
        + f.owner.len()
        + f.pidx.len()
        + f.fidx.len()
        + f.fld.len();
    println!(
        "files {}  vars {}  sites {}  facts {}",
        ctx.units.len(),
        ctx.model.var_count(),
        ctx.model.sites,
        nfacts
    );
    println!(
        "analyze {:.0?}  extract {:.0?}  solve {:.2?}  check {:.2?}  model: bits {} dep {}",
        run.analyze_time,
        run.extract_time,
        run.solve_time,
        run.check_time,
        ctx.solution.bits.len(),
        ctx.solution.dep.len()
    );
    for (path, err) in run.failed {
        println!(
            "not analyzed: {}: {}",
            path,
            err.lines().next().unwrap_or("")
        );
    }

    let mut all = Vec::new();
    for u in ctx.units {
        lambdas(&u.hir, u.id, &mut all);
    }
    let lines: HashMap<(u32, u32), usize> = all.iter().map(|l| ((l.unit, l.id), l.line)).collect();

    let mut tally: BTreeMap<Verdict, usize> = BTreeMap::new();
    let mut rows: Vec<(Verdict, String)> = Vec::new();
    for l in &all {
        let Some(v) = ctx.model.find(&VarKey::Lam(l.unit, l.id)) else {
            continue;
        };
        let s_bits = bits_of.get(&v).copied().unwrap_or(0);
        let mut s_prop = 0u32;
        let mut free = Vec::new();
        for &w in deps_of.get(&v).map(|d| d.as_slice()).unwrap_or(&[]) {
            match ctx.model.keys_by_id[w as usize] {
                VarKey::Param(o, i) if o == v => s_prop |= 1 << i,
                _ => free.push(ctx.describe(w, &lines)),
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
        if verdict != Verdict::Same || run.all {
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
                    ctx.describe(v, &lines),
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
        if run.all || *v != Verdict::Tighter || *n <= 15 {
            println!("{row}");
        }
    }

    println!("-- module exports called through an instance");
    let mut called: Vec<VarId> = ctx
        .model
        .site_callee
        .values()
        .copied()
        .filter(|v| matches!(ctx.model.keys_by_id[*v as usize], VarKey::Inst(..)))
        .collect();
    called.sort();
    called.dedup();
    for v in called.iter().take(if run.all { usize::MAX } else { 25 }) {
        let deps: Vec<String> = deps_of
            .get(v)
            .map(|d| d.iter().map(|w| ctx.describe(*w, &lines)).collect())
            .unwrap_or_default();
        println!(
            "  {} = {} {}",
            ctx.describe(*v, &lines),
            fmt_bits(bits_of.get(v).copied().unwrap_or(0)),
            if deps.is_empty() {
                String::new()
            } else {
                format!("+ {{{}}}", deps.join(", "))
            }
        );
    }
    println!("  ({} distinct exports called)", called.len());

    println!("-- module shapes and top levels");
    for u in ctx.units {
        let shape = match ctx.shapes.get(&u.path) {
            Some(ModuleShape::Lambda { exports, .. }) => {
                format!("lambda, {} exports", exports.len())
            }
            Some(ModuleShape::Struct { exports }) => format!("struct, {} exports", exports.len()),
            _ => "unknown".to_string(),
        };
        let top = ctx
            .model
            .find(&VarKey::Top(u.id))
            .and_then(|t| bits_of.get(&t).copied())
            .unwrap_or(0);
        if run.all || top != 0 {
            println!(
                "  {}: {}; top level raises {}",
                ctx.file(u.id),
                shape,
                fmt_bits(top)
            );
        }
    }

    if !ctx.solution.viol.is_empty() {
        println!("-- ceilings the solver finds exceeded");
        let mut by: BTreeMap<VarId, u64> = BTreeMap::new();
        for &(v, b) in &ctx.solution.viol {
            *by.entry(v).or_default() |= crate::model::to_word(std::iter::once(b));
        }
        for (v, b) in by {
            println!(
                "  {} exceeds its ceiling by {}",
                ctx.describe(v, &lines),
                fmt_bits(b)
            );
        }
    }

    if run.expect {
        crate::expect::check(ctx, &all, &bits_of, &deps_of)
    } else {
        0
    }
}
