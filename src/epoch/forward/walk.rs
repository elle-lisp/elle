// audited: 2026-11-20
//! The epoch 18 letrec walk: the runs of defs each body holds, and defs that read their own name.
//!
//! docs/epochs/bodies.md
//!
//! [`super::reading`] reads each body. A run becomes a letrec over the rest of
//! its body, unless a letrec cannot hold it or would change what a read finds.
//! A def outside every run whose value reads its own name directly becomes a
//! let over the rest of its body.

use super::reading::{Form, Reading, Slot};
use super::scan::value_reads;
use super::{Binder, Left, Plan};
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

struct Walker {
    plan: Plan,
}

impl Walker {
    /// Read the forms of `items` from `start` on, a body whose parent stands
    /// at `path`.
    fn body(&mut self, path: &[usize], items: &[Syntax], start: usize) {
        let Some(Reading {
            slots,
            read,
            forward,
            runs,
        }) = Reading::of(items, start)
        else {
            return;
        };
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
