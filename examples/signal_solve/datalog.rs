// audited: 2026-10-05
//! Write the facts and rules as z3 fixedpoint SMT-LIB, run z3, and read the least model back.
//!
//! docs/signals/inference.md

use crate::link::Lowered;
use crate::model::{all_bits, Model};
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::io::Write as _;

/// The least model z3 computed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Solution {
    pub bits: BTreeSet<(u32, u32)>,
    pub dep: BTreeSet<(u32, u32)>,
    pub viol: BTreeSet<(u32, u32)>,
}

/// The rules, independent of any program. `c` is a context, `g` a callee,
/// `o` the owner whose parameters a call binds, `w` a free variable, `a` what
/// a call binds it to.
///
/// A call contributes the callee's own bits, the callee's free variables the
/// call does not bind, and for each one it binds, the signal of what it binds
/// it to. A free variable bound to nothing known contributes every bit.
const RULES: &str = r#"
(rule (=> (and (call c g o) (bits g b)) (raw c b)))
(rule (=> (and (call c g o) (dep g w) (not (owner w o))) (rdep c w)))
(rule (=> (and (sub c g w a) (dep g w) (bits a b)) (raw c b)))
(rule (=> (and (sub c g w a) (dep g w) (dep a x)) (rdep c x)))
(rule (=> (and (miss c g w) (dep g w) (all b)) (raw c b)))
(rule (=> (and (flow c a) (bits a b)) (raw c b)))
(rule (=> (and (flow c a) (dep a x)) (rdep c x)))
(rule (=> (and (raw c b) (not (muf c b)) (not (ceiled c))) (bits c b)))
(rule (=> (ceil c b) (bits c b)))
(rule (=> (and (rdep c w) (not (ceiled c))) (dep c w)))
(rule (=> (and (sqof q f) (bits f b) (not (sqm q b))) (bits q b)))
(rule (=> (and (sqof q f) (bits f b) (sqm q b)) (bits q #b0000000)))
(rule (=> (and (sqof q f) (dep f w)) (dep q w)))
(rule (=> (and (raw c b) (ceiled c) (not (ceil c b)) (not (muf c b))) (viol c b)))
"#;

fn width(n: usize) -> usize {
    let mut w = 1;
    while (1usize << w) <= n {
        w += 1;
    }
    w.max(4)
}

/// Write the program. `reverse` emits every fact list in reverse order, which
/// is how the determinism check perturbs the input.
pub fn program(model: &Model, lowered: &Lowered, reverse: bool) -> String {
    let f = &model.facts;
    let vw = width(model.var_count());
    let v = |x: u32| format!("(_ bv{} {})", x, vw);
    let b = |x: u32| format!("(_ bv{} 7)", x);

    let mut out = String::new();
    writeln!(out, "(set-option :fp.engine datalog)").unwrap();
    writeln!(out, "(define-sort V () (_ BitVec {}))", vw).unwrap();
    writeln!(out, "(define-sort B () (_ BitVec 7))").unwrap();
    for (name, sorts) in [
        ("raw", "V B"),
        ("bits", "V B"),
        ("dep", "V V"),
        ("rdep", "V V"),
        ("call", "V V V"),
        ("sub", "V V V V"),
        ("miss", "V V V"),
        ("flow", "V V"),
        ("owner", "V V"),
        ("muf", "V B"),
        ("ceiled", "V"),
        ("ceil", "V B"),
        ("sqof", "V V"),
        ("sqm", "V B"),
        ("all", "B"),
        ("viol", "V B"),
    ] {
        writeln!(out, "(declare-rel {} ({}))", name, sorts).unwrap();
    }
    for name in ["c", "g", "o", "w", "a", "x", "f", "q"] {
        writeln!(out, "(declare-var {} V)", name).unwrap();
    }
    writeln!(out, "(declare-var b B)").unwrap();
    out.push_str(RULES);

    let mut facts: Vec<String> = Vec::new();
    for bit in all_bits() {
        facts.push(format!("(all {})", b(bit)));
    }
    facts.extend(
        f.raw
            .iter()
            .map(|&(c, x)| format!("(raw {} {})", v(c), b(x))),
    );
    facts.extend(
        f.bits
            .iter()
            .map(|&(c, x)| format!("(bits {} {})", v(c), b(x))),
    );
    facts.extend(
        f.dep
            .iter()
            .map(|&(c, w)| format!("(dep {} {})", v(c), v(w))),
    );
    facts.extend(
        f.owner
            .iter()
            .map(|&(w, p)| format!("(owner {} {})", v(w), v(p))),
    );
    facts.extend(
        lowered
            .call
            .iter()
            .map(|&(c, g, o)| format!("(call {} {} {})", v(c), v(g), v(o))),
    );
    facts.extend(
        lowered
            .sub
            .iter()
            .map(|&(c, g, w, a)| format!("(sub {} {} {} {})", v(c), v(g), v(w), v(a))),
    );
    facts.extend(
        lowered
            .miss
            .iter()
            .map(|&(c, g, w)| format!("(miss {} {} {})", v(c), v(g), v(w))),
    );
    facts.extend(
        lowered
            .flow
            .iter()
            .map(|&(c, a)| format!("(flow {} {})", v(c), v(a))),
    );
    facts.extend(
        f.muf
            .iter()
            .map(|&(c, x)| format!("(muf {} {})", v(c), b(x))),
    );
    facts.extend(f.ceiled.iter().map(|&c| format!("(ceiled {})", v(c))));
    facts.extend(
        f.ceil
            .iter()
            .map(|&(c, x)| format!("(ceil {} {})", v(c), b(x))),
    );
    facts.extend(
        f.sqof
            .iter()
            .map(|&(q, t)| format!("(sqof {} {})", v(q), v(t))),
    );
    facts.extend(
        f.sqm
            .iter()
            .map(|&(q, x)| format!("(sqm {} {})", v(q), b(x))),
    );
    if reverse {
        facts.reverse();
    }
    for fact in facts {
        writeln!(out, "(rule {})", fact).unwrap();
    }
    for rel in ["bits", "dep", "viol"] {
        writeln!(out, "(query {} :print-answer true)", rel).unwrap();
    }
    out
}

/// Run z3 over a program and read back the three queried relations.
pub fn solve(program: &str) -> Result<Solution, String> {
    let mut file = tempfile::Builder::new()
        .prefix("signal-solve-")
        .suffix(".smt2")
        .tempfile()
        .map_err(|e| format!("temp file: {e}"))?;
    file.write_all(program.as_bytes())
        .map_err(|e| format!("write: {e}"))?;
    let out = std::process::Command::new("z3")
        .arg(file.path())
        .output()
        .map_err(|e| format!("run z3: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    if !out.status.success() && !text.starts_with("sat") && !text.starts_with("unsat") {
        return Err(format!(
            "z3 failed: {}{}",
            text,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let answers = split_answers(&text)?;
    if answers.len() != 3 {
        return Err(format!(
            "expected 3 answers, got {}: {}",
            answers.len(),
            text
        ));
    }
    Ok(Solution {
        bits: pairs(&answers[0])?,
        dep: pairs(&answers[1])?,
        viol: pairs(&answers[2])?,
    })
}

/// Split z3's output into one formula per query. An `unsat` answer is an empty
/// relation.
fn split_answers(text: &str) -> Result<Vec<String>, String> {
    let mut answers: Vec<String> = Vec::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        let t = line.trim();
        if t == "sat" || t == "unsat" {
            if let Some(c) = current.take() {
                answers.push(c);
            }
            current = Some(String::new());
            if t == "unsat" {
                answers.push(String::new());
                current = None;
            }
        } else if let Some(c) = current.as_mut() {
            c.push_str(t);
            c.push(' ');
        } else if !t.is_empty() {
            return Err(format!("unexpected z3 output: {t}"));
        }
    }
    if let Some(c) = current.take() {
        answers.push(c);
    }
    Ok(answers)
}

/// Read a binary relation's tuples out of a formula of the shape
/// `(or (and (= (:var 0) X) (= (:var 1) Y)) ...)`. Any other shape is an error,
/// because a compressed answer would silently drop tuples.
fn pairs(formula: &str) -> Result<BTreeSet<(u32, u32)>, String> {
    let mut out = BTreeSet::new();
    let f = formula.trim();
    if f.is_empty() || f == "false" {
        return Ok(out);
    }
    let mut vals: Vec<(u32, u32)> = Vec::new();
    let mut rest = f;
    while let Some(pos) = rest.find("(= (:var ") {
        rest = &rest[pos + "(= (:var ".len()..];
        let close = rest.find(')').ok_or("unterminated var")?;
        let var: u32 = rest[..close].parse().map_err(|_| "bad var index")?;
        rest = rest[close + 1..].trim_start();
        let end = rest.find(')').ok_or("unterminated value")?;
        let value = parse_bv(rest[..end].trim())?;
        vals.push((var, value));
        rest = &rest[end + 1..];
    }
    let structural = f.matches("(=").count();
    if structural != vals.len() || !vals.len().is_multiple_of(2) {
        return Err(format!(
            "unexpected answer shape: {}",
            &f[..f.len().min(400)]
        ));
    }
    for pair in vals.chunks(2) {
        match pair {
            [(0, a), (1, b)] => {
                out.insert((*a, *b));
            }
            _ => return Err(format!("unexpected tuple order: {pair:?}")),
        }
    }
    Ok(out)
}

fn parse_bv(s: &str) -> Result<u32, String> {
    if let Some(h) = s.strip_prefix("#x") {
        u32::from_str_radix(h, 16).map_err(|e| format!("{s}: {e}"))
    } else if let Some(b) = s.strip_prefix("#b") {
        u32::from_str_radix(b, 2).map_err(|e| format!("{s}: {e}"))
    } else if let Some(rest) = s.strip_prefix("(_ bv") {
        rest.split_whitespace()
            .next()
            .ok_or("empty bv")?
            .parse()
            .map_err(|_| format!("bad bv {s}"))
    } else {
        Err(format!("unknown value form {s}"))
    }
}
