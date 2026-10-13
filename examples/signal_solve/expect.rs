// audited: 2026-10-12
//! Check the `# expect` and `# violates` lines in the analyzed files against the solver's least model.
//!
//! docs/impl/solver.md

use crate::model::{VarId, VarKey};
use crate::report::{Context, Lambda};
use std::collections::HashMap;

/// Expectation markers in the field list: a free import export, and a free
/// module instantiation.
const IMPORT: u32 = u32::MAX;
const INSTANTIATE: u32 = u32::MAX - 1;

/// A signal as a line states it, or as the model answers it: bits, own
/// parameters, and the other free variables by name.
#[derive(Debug, Default, PartialEq, Eq)]
struct Answer {
    bits: u64,
    prop: u32,
    fields: Vec<(u32, String)>,
    /// Free variables a line cannot name.
    other: usize,
}

impl Answer {
    fn show(&self) -> String {
        format!(
            "{} prop {:b} fields {:?}{}",
            crate::report::fmt_bits(self.bits),
            self.prop,
            self.fields,
            if self.other > 0 {
                format!(" + {} other free", self.other)
            } else {
                String::new()
            }
        )
    }
}

/// The bits a `|:kw …|` literal names, and the text after it.
fn parse_bits(rest: &str) -> (u64, &str) {
    let rest = rest.trim().strip_prefix('|').expect("bits open with |");
    let (lit, tail) = rest.split_once('|').expect("bits close with |");
    let reg = elle::signals::registry::global_registry().lock().unwrap();
    let mut bits = 0u64;
    for kw in lit.split_whitespace() {
        let kw = kw.trim_start_matches(':');
        bits |= reg.to_signal_bits(kw).expect("known signal").raw();
    }
    (bits, tail)
}

/// The answer a line states after its name: `|bits| [prop I] [field I KEY]
/// [import KEY] [instantiate]`.
fn parse_answer(rest: &str) -> Answer {
    let (bits, tail) = parse_bits(rest);
    let mut want = Answer {
        bits,
        ..Answer::default()
    };
    let toks: Vec<&str> = tail.split_whitespace().collect();
    let mut t = 0;
    while t < toks.len() {
        match toks[t] {
            "prop" => {
                want.prop |= 1 << toks[t + 1].parse::<u32>().unwrap();
                t += 2;
            }
            // An open import's export, by key.
            "import" => {
                want.fields.push((IMPORT, toks[t + 1].to_string()));
                t += 2;
            }
            // An open module instantiation.
            "instantiate" => {
                want.fields.push((INSTANTIATE, String::new()));
                t += 1;
            }
            "field" => {
                want.fields
                    .push((toks[t + 1].parse().unwrap(), toks[t + 2].to_string()));
                t += 3;
            }
            other => panic!("unknown expect token {other}"),
        }
    }
    want.fields.sort();
    want
}

/// The answer the model gives for `v`: `bits`, and its free variables `free`
/// named as a line names them.
fn answer_of(ctx: &Context, v: VarId, bits: u64, free: &[VarId]) -> Answer {
    let mut got = Answer {
        bits: bits & elle::signals::CAP_MASK.raw(),
        ..Answer::default()
    };
    for &w in free {
        match ctx.model.keys_by_id[w as usize] {
            VarKey::Param(o, i) if o == v => got.prop |= 1 << i,
            VarKey::Field(p, k) => match ctx.model.keys_by_id[p as usize] {
                VarKey::Param(_, i) => got
                    .fields
                    .push((i, ctx.model.key_names[k as usize].clone())),
                _ => got.other += 1,
            },
            VarKey::Inst(_, k) => got
                .fields
                .push((IMPORT, ctx.model.key_names[k as usize].clone())),
            VarKey::Obj(_) => got.fields.push((INSTANTIATE, String::new())),
            _ => got.other += 1,
        }
    }
    got.fields.sort();
    got
}

/// The variable of the lambda bound to `name` in file `unit`.
fn lambda_named(ctx: &Context, all: &[Lambda], unit: u32, name: &str) -> Option<VarId> {
    let l = all.iter().find(|l| {
        l.unit == unit
            && ctx
                .names
                .get(&(l.unit, l.id))
                .and_then(|s| ctx.symbols.name(*s))
                == Some(name)
    })?;
    ctx.model.find(&VarKey::Lam(l.unit, l.id))
}

/// Check every `# expect NAME ANSWER` line in the analyzed files against the
/// solver's answer for the lambda bound to NAME in that file. The parameters,
/// fields and bits must match exactly. Then check that the `# violates NAME
/// ANSWER` lines state exactly how the model finds each ceiling in those files
/// broken: the bits past it, and the free variables under it. Returns the
/// number of failures.
pub fn check(
    ctx: &Context,
    all: &[Lambda],
    bits_of: &HashMap<VarId, u64>,
    deps_of: &HashMap<VarId, Vec<VarId>>,
) -> usize {
    let mut viol_of: HashMap<VarId, u64> = HashMap::new();
    for &(v, b) in &ctx.solution.viol {
        *viol_of.entry(v).or_default() |= crate::model::to_word(std::iter::once(b));
    }
    let mut vdep_of: HashMap<VarId, Vec<VarId>> = HashMap::new();
    for &(v, w) in &ctx.solution.vdep {
        vdep_of.entry(v).or_default().push(w);
    }
    let mut failures = 0;
    for u in ctx.units {
        let src = std::fs::read_to_string(&u.path).unwrap_or_default();
        let mut stated: HashMap<VarId, Answer> = HashMap::new();
        for line in src.lines() {
            let (spec, violates) = if let Some(s) = line.trim().strip_prefix("# violates ") {
                (s, true)
            } else if let Some(s) = line.trim().strip_prefix("# expect ") {
                (s, false)
            } else {
                continue;
            };
            let (name, rest) = spec.split_once(' ').expect("NAME |bits|");
            let want = parse_answer(rest);
            let Some(v) = lambda_named(ctx, all, u.id, name) else {
                println!("FAIL {}: no lambda named {}", ctx.file(u.id), name);
                failures += 1;
                continue;
            };
            if violates {
                stated.insert(v, want);
                continue;
            }
            let free = deps_of.get(&v).map(|d| d.as_slice()).unwrap_or(&[]);
            let got = answer_of(ctx, v, bits_of.get(&v).copied().unwrap_or(0), free);
            let ok = got == want;
            if !ok {
                failures += 1;
            }
            println!(
                "{} {}:{}: want {}; got {}",
                if ok { "ok  " } else { "FAIL" },
                ctx.file(u.id),
                name,
                want.show(),
                got.show(),
            );
        }
        // Every violation in this file, stated or found, must be both.
        let mut lambdas: Vec<VarId> = all
            .iter()
            .filter(|l| l.unit == u.id)
            .filter_map(|l| ctx.model.find(&VarKey::Lam(l.unit, l.id)))
            .filter(|v| {
                stated.contains_key(v) || viol_of.contains_key(v) || vdep_of.contains_key(v)
            })
            .collect();
        lambdas.sort();
        for v in lambdas {
            let want = stated.remove(&v).unwrap_or_default();
            let free = vdep_of.get(&v).map(|d| d.as_slice()).unwrap_or(&[]);
            let got = answer_of(ctx, v, viol_of.get(&v).copied().unwrap_or(0), free);
            let ok = got == want;
            if !ok {
                failures += 1;
            }
            println!(
                "{} {}: violates want {}; got {}",
                if ok { "ok  " } else { "FAIL" },
                ctx.describe_var(v, all),
                want.show(),
                got.show(),
            );
        }
    }
    failures
}
