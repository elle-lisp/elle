// audited: 2026-11-20
//! A run of defs outside a body: whether it moves into a let that takes in the rest of its sequence, or stays.
//!
//! docs/epochs/bodies.md
//!
//! The run moves unless a read would then find another binding: a value that
//! reads a name the let binds only after it, or a read in the body around the
//! sequence that the let would no longer reach.

use super::super::reads::{body_defs, bound, Region, Scan};
use super::super::{Def, Note, Placement};
use super::{Body, Walker};
use crate::syntax::Syntax;

impl<'t> Walker<'t> {
    /// Walk the children of `parent` from `from` up to `end`, a sequence that
    /// stands outside a body. Each run of defs there moves into a let that
    /// takes in the rest, or stays, and the walk goes on after it. `replaces`
    /// when the let may take the parent's place: the parent is a `begin`.
    pub(super) fn sequence(&mut self, parent: &'t Syntax, from: usize, end: usize, replaces: bool) {
        let items = parent.kind.children();
        let mut i = from;
        while i < end {
            let Some(def) = Def::of(&items[i]) else {
                self.at(i, |w| w.form(&items[i]));
                i += 1;
                continue;
            };
            if let Err(reason) = def.names() {
                self.note(def, reason.to_string());
                self.at(i, |w| w.def_parts(def));
                i += 1;
                continue;
            }
            let run = items[i..end]
                .iter()
                .take_while(|n| Def::of(n).is_some_and(|d| d.names().is_ok()))
                .count();
            let defs: Vec<Def<'t>> = items[i..i + run].iter().filter_map(Def::of).collect();
            let region = Region {
                parent,
                first: i,
                end,
            };
            match self.verdict(&defs, &items[i + run..end], region) {
                Ok(()) => {
                    self.plan.placements.push(Placement {
                        path: self.path.clone(),
                        first: i,
                        count: run,
                        end,
                        replaces: replaces && i == from,
                    });
                    for (k, def) in defs.iter().enumerate() {
                        self.at(i + k, |w| w.def_parts(*def));
                    }
                    for (k, item) in items.iter().enumerate().take(end).skip(i + run) {
                        self.at(k, |w| w.body_form(item, false));
                    }
                    return;
                }
                Err(reason) => {
                    for (k, def) in defs.iter().enumerate() {
                        self.note(*def, reason.to_string());
                        self.at(i + k, |w| w.def_parts(*def));
                    }
                    i += run;
                }
            }
        }
    }

    /// Whether the run `defs`, with `body` after it, may move into a let over
    /// `region`, or why it stays.
    fn verdict(
        &self,
        defs: &[Def<'t>],
        body: &[Syntax],
        region: Region<'t>,
    ) -> Result<(), &'static str> {
        let last = defs.last().expect("a run holds a def");
        if body.is_empty() && last.binds_pattern() {
            return Err("it binds a pattern, and no form after it gives the let its value");
        }
        let names: Vec<Vec<&str>> = defs.iter().map(|d| d.names().unwrap_or_default()).collect();
        for (k, def) in defs.iter().enumerate() {
            if def.value_reads(&names[k..].concat()) {
                return Err(
                    "its value, or one before it in the let, reads a name the let binds only after",
                );
            }
        }
        let mut all = names.concat();
        all.extend(body_defs(body));
        match self.bodies.last() {
            Some(home) if outside(home, region, &all) => {
                Err("a read of its name outside the let would no longer find it")
            }
            _ => Ok(()),
        }
    }

    /// Record that the rule leaves `def` as written, for `reason`.
    pub(super) fn note(&mut self, def: Def<'t>, reason: String) {
        self.plan.notes.push(Note {
            span: def.node.span,
            reason,
        });
    }
}

/// Whether `home` reads one of `names` outside `region`: before it, in the
/// form that holds it, or after it up to a def in `home` that binds the name
/// again. That def's own value still counts. Before the form, a def in `home`
/// binds its name again for the forms after it, as a def in a sequence does.
fn outside(home: &Body<'_>, region: Region<'_>, names: &[&str]) -> bool {
    let scan = Scan { skip: Some(region) };
    let before = home.forms[..home.at].iter().copied();
    if scan.in_order(before, &mut names.to_vec()) || scan.reads(home.forms[home.at], names) {
        return true;
    }
    let mut live = names.to_vec();
    for form in &home.forms[home.at + 1..] {
        let Some(def) = Def::of(form) else {
            if Scan::PLAIN.reads(form, &live) {
                return true;
            }
            continue;
        };
        if def.value_reads(&live) {
            return true;
        }
        let again = bound(def.name()).unwrap_or_default();
        live.retain(|n| !again.contains(n));
        if live.is_empty() {
            return false;
        }
    }
    false
}
