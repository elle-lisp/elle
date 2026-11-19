// audited: 2026-11-19
//! The epoch 18 letrec walk: the forward reads the moves leave in each body, the runs of defs they make, and defs that read their name.
//!
//! docs/epochs.md
//!
//! A forward read in form `i` of a name whose first def is form `j` asks for
//! the run of forms `i` through `j`, and runs that overlap join. A run becomes
//! a letrec over the rest of its body, unless a letrec cannot hold it or
//! would change what a read finds. A def outside every run whose value reads
//! its own name directly becomes a let over the rest of its body.

use super::scan::{form_reads, value_reads, Read};
use super::{Binder, Left, Plan};
use crate::epoch::placement::{is_begin, Def};
use crate::syntax::Syntax;

/// Walk a file's forms and every body in them, and answer the plan.
pub(super) fn over(forms: &[Syntax]) -> Plan {
    let mut walker = Walker {
        plan: Plan {
            binders: Vec::new(),
            left: Vec::new(),
        },
    };
    super::bodies::each(forms, &mut |path, items, start| {
        walker.body(path, items, start)
    });
    walker.plan
}

/// One form of a body, through every `begin` in it.
struct Slot<'t> {
    node: &'t Syntax,
    /// Its index among the forms of the body's parent.
    index: usize,
    /// Whether it stands there itself, not inside a `begin`.
    direct: bool,
}

/// A form of a body read as a def: the def, and the names it binds.
struct Form<'t> {
    def: Option<Def<'t>>,
    names: Vec<&'t str>,
}

struct Walker {
    plan: Plan,
}

impl Walker {
    /// Read the forms of `items` from `start` on, a body whose parent stands
    /// at `path`.
    fn body(&mut self, path: &[usize], items: &[Syntax], start: usize) {
        let Some(forms) = items.get(start..) else {
            return;
        };
        let slots = slots(forms, start);
        let read: Vec<Form<'_>> = slots
            .iter()
            .map(|s| {
                let def = Def::of(s.node);
                let names = def.and_then(|d| d.names().ok()).unwrap_or_default();
                Form { def, names }
            })
            .collect();
        // Where each name's first def stands.
        let mut first_def: Vec<(&str, usize)> = Vec::new();
        for (k, form) in read.iter().enumerate() {
            for name in &form.names {
                if !first_def.iter().any(|(n, _)| n == name) {
                    first_def.push((name, k));
                }
            }
        }
        let mut forward: Vec<(usize, usize, Read<'_>)> = Vec::new();
        for (i, slot) in slots.iter().enumerate() {
            let later: Vec<&str> = first_def
                .iter()
                .filter(|(_, j)| *j > i)
                .map(|(n, _)| *n)
                .collect();
            for read in form_reads(slot.node, &later) {
                if read.in_lambda {
                    let j = first_def.iter().find(|(n, _)| *n == read.name).map(|p| p.1);
                    forward.push((i, j.expect("a later name has a def"), read));
                }
            }
        }
        let runs = joined(forward.iter().map(|(i, j, _)| (*i, *j)).collect());
        let mut binders = Vec::new();
        for &(a, b) in &runs {
            match verdict(&slots, &read, a, b) {
                Ok(()) => binders.push(Binder {
                    path: path.to_vec(),
                    first: slots[a].index,
                    count: b - a + 1,
                    head: "letrec",
                }),
                Err(reason) => {
                    for (_, _, read) in forward.iter().filter(|(i, ..)| (a..=b).contains(i)) {
                        self.plan.left.push(Left {
                            span: read.at,
                            message: format!(
                                "'{}' is read before its def, and the epoch 18 rule leaves the read: {}",
                                read.name, reason
                            ),
                            refused: true,
                        });
                    }
                }
            }
        }
        for (k, slot) in slots.iter().enumerate() {
            if runs.iter().any(|(a, b)| (*a..=*b).contains(&k)) {
                continue;
            }
            let Some(def) = read[k].def else {
                continue;
            };
            let own = value_reads(def, &read[k].names);
            let Some(direct) = own.iter().find(|r| !r.in_lambda) else {
                continue;
            };
            let reason = if own.iter().any(|r| r.in_lambda) {
                "its value reads it both directly and inside a lambda"
            } else if !slot.direct {
                "the def stands in a begin, whose defs bind for the forms after it"
            } else {
                binders.push(Binder {
                    path: path.to_vec(),
                    first: slot.index,
                    count: 1,
                    head: "let",
                });
                continue;
            };
            self.plan.left.push(Left {
                span: def.node.span,
                message: format!(
                    "the epoch 18 rule leaves this def of '{}': {}",
                    direct.name, reason
                ),
                refused: false,
            });
        }
        binders.sort_by_key(|b| b.first);
        self.plan.binders.extend(binders);
    }
}

/// The forms of a body that starts at `start` in its parent, each `begin`
/// replaced by its forms.
fn slots(forms: &[Syntax], start: usize) -> Vec<Slot<'_>> {
    fn inside<'t>(node: &'t Syntax, index: usize, out: &mut Vec<Slot<'t>>) {
        for child in &node.kind.children()[1..] {
            if is_begin(child) {
                inside(child, index, out);
            } else {
                out.push(Slot {
                    node: child,
                    index,
                    direct: false,
                });
            }
        }
    }
    let mut out = Vec::new();
    for (k, form) in forms.iter().enumerate() {
        if is_begin(form) {
            inside(form, start + k, &mut out);
        } else {
            out.push(Slot {
                node: form,
                index: start + k,
                direct: true,
            });
        }
    }
    out
}

/// The spans `(i, j)` of each forward read, sorted, with those that overlap
/// joined into one.
fn joined(mut spans: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    spans.sort_unstable();
    let mut out: Vec<(usize, usize)> = Vec::new();
    for (a, b) in spans {
        match out.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

/// Whether the forms `a` through `b` may become one letrec, or why not.
fn verdict(slots: &[Slot<'_>], read: &[Form<'_>], a: usize, b: usize) -> Result<(), String> {
    let mut defs = Vec::new();
    for k in a..=b {
        if !slots[k].direct {
            return Err("the run stands in a begin, whose defs bind for the forms after it".into());
        }
        let Some(def) = read[k].def else {
            return Err("the run holds a form other than a def".into());
        };
        if def.binds_pattern() || read[k].names.len() != 1 {
            return Err("the run holds a def that binds a pattern".into());
        }
        defs.push(def);
    }
    let names: Vec<&str> = (a..=b).map(|k| read[k].names[0]).collect();
    for (k, name) in names.iter().enumerate() {
        if names[..k].contains(name) {
            return Err(format!("the run binds '{name}' twice"));
        }
    }
    for (k, def) in defs.iter().enumerate() {
        if let Some(direct) = value_reads(*def, &names[k..]).iter().find(|r| !r.in_lambda) {
            return Err(format!(
                "the value of '{}' reads '{}' before the letrec binds it",
                names[k], direct.name
            ));
        }
    }
    // A def before the run binds the name too, and a form of the run before
    // the run's def reads that earlier binding.
    let earlier: Vec<&str> = read[..a].iter().flat_map(|f| f.names.clone()).collect();
    for (m, name) in names.iter().enumerate() {
        if earlier.contains(name)
            && defs[..m]
                .iter()
                .any(|d| !value_reads(*d, &[name]).is_empty())
        {
            return Err(format!(
                "the letrec would move a read of '{name}' to another def"
            ));
        }
    }
    Ok(())
}
