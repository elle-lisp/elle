// audited: 2026-10-05
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

/// Check every `# expect NAME |bits| [prop I] [field I KEY] [import KEY]
/// [instantiate]` line in the analyzed files against the solver's answer for
/// the lambda bound to NAME in that file. The parameters, fields and bits must
/// match exactly. Then check that the `# violates NAME |bits|` lines state
/// exactly the ceilings the model finds exceeded in those files. Returns the
/// number of failures.
pub fn check(
    ctx: &Context,
    all: &[Lambda],
    bits_of: &HashMap<VarId, u64>,
    deps_of: &HashMap<VarId, Vec<VarId>>,
) -> usize {
    let cap = elle::signals::CAP_MASK.raw();
    let mut viol_of: HashMap<VarId, u64> = HashMap::new();
    for &(v, b) in &ctx.solution.viol {
        *viol_of.entry(v).or_default() |= crate::model::to_word(std::iter::once(b));
    }
    let mut failures = 0;
    for u in ctx.units {
        let src = std::fs::read_to_string(&u.path).unwrap_or_default();
        let mut stated: HashMap<VarId, u64> = HashMap::new();
        for line in src.lines() {
            if let Some(spec) = line.trim().strip_prefix("# violates ") {
                let (name, rest) = spec.split_once(' ').expect("violates NAME |bits|");
                let (want, _) = parse_bits(rest);
                match lambda_named(ctx, all, u.id, name) {
                    Some(v) => {
                        stated.insert(v, want);
                    }
                    None => {
                        println!("FAIL {}: no lambda named {}", ctx.file(u.id), name);
                        failures += 1;
                    }
                }
                continue;
            }
            let Some(spec) = line.trim().strip_prefix("# expect ") else {
                continue;
            };
            let (name, rest) = spec.split_once(' ').expect("expect NAME |bits|");
            let (want_bits, tail) = parse_bits(rest);
            let mut want_prop = 0u32;
            let mut want_fields: Vec<(u32, String)> = Vec::new();
            let toks: Vec<&str> = tail.split_whitespace().collect();
            let mut t = 0;
            while t < toks.len() {
                match toks[t] {
                    "prop" => {
                        want_prop |= 1 << toks[t + 1].parse::<u32>().unwrap();
                        t += 2;
                    }
                    // An open import's export, by key.
                    "import" => {
                        want_fields.push((IMPORT, toks[t + 1].to_string()));
                        t += 2;
                    }
                    // An open module instantiation.
                    "instantiate" => {
                        want_fields.push((INSTANTIATE, String::new()));
                        t += 1;
                    }
                    "field" => {
                        want_fields.push((toks[t + 1].parse().unwrap(), toks[t + 2].to_string()));
                        t += 3;
                    }
                    other => panic!("unknown expect token {other}"),
                }
            }
            let Some(v) = lambda_named(ctx, all, u.id, name) else {
                println!("FAIL {}: no lambda named {}", ctx.file(u.id), name);
                failures += 1;
                continue;
            };
            let got_bits = bits_of.get(&v).copied().unwrap_or(0) & cap;
            let mut got_prop = 0u32;
            let mut got_fields: Vec<(u32, String)> = Vec::new();
            let mut other_free = 0;
            for &w in deps_of.get(&v).map(|d| d.as_slice()).unwrap_or(&[]) {
                match ctx.model.keys_by_id[w as usize] {
                    VarKey::Param(o, i) if o == v => got_prop |= 1 << i,
                    VarKey::Field(p, k) => match ctx.model.keys_by_id[p as usize] {
                        VarKey::Param(_, i) => {
                            got_fields.push((i, ctx.model.key_names[k as usize].clone()))
                        }
                        _ => other_free += 1,
                    },
                    VarKey::Inst(_, k) => {
                        got_fields.push((IMPORT, ctx.model.key_names[k as usize].clone()))
                    }
                    VarKey::Obj(_) => got_fields.push((INSTANTIATE, String::new())),
                    _ => other_free += 1,
                }
            }
            got_fields.sort();
            want_fields.sort();
            let ok = got_bits == want_bits
                && got_prop == want_prop
                && got_fields == want_fields
                && other_free == 0;
            if !ok {
                failures += 1;
            }
            println!(
                "{} {}:{}: want {} prop {:b} fields {:?}; got {} prop {:b} fields {:?}{}",
                if ok { "ok  " } else { "FAIL" },
                ctx.file(u.id),
                name,
                crate::report::fmt_bits(want_bits),
                want_prop,
                want_fields,
                crate::report::fmt_bits(got_bits),
                got_prop,
                got_fields,
                if other_free > 0 {
                    format!(" + {} other free", other_free)
                } else {
                    String::new()
                }
            );
        }
        // Every violation in this file, stated or found, must be both.
        let mut lambdas: Vec<VarId> = all
            .iter()
            .filter(|l| l.unit == u.id)
            .filter_map(|l| ctx.model.find(&VarKey::Lam(l.unit, l.id)))
            .filter(|v| stated.contains_key(v) || viol_of.contains_key(v))
            .collect();
        lambdas.sort();
        for v in lambdas {
            let want = stated.get(&v).copied().unwrap_or(0);
            let got = viol_of.get(&v).copied().unwrap_or(0) & cap;
            let ok = want == got;
            if !ok {
                failures += 1;
            }
            println!(
                "{} {}: violates want {}; got {}",
                if ok { "ok  " } else { "FAIL" },
                ctx.describe_var(v, all),
                crate::report::fmt_bits(want),
                crate::report::fmt_bits(got),
            );
        }
    }
    failures
}
