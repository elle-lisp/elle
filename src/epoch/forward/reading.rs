// audited: 2026-11-20
//! One body as the epoch 18 rule reads it: its forms through each begin, the forward reads in them, their runs, and the begins to splice.
//!
//! docs/epochs/bodies.md
//!
//! A forward read in form `i` of a name whose first def is form `j` asks for
//! the run of forms `i` through `j`, and runs that overlap join. Before the
//! moves, each begin that holds a form of a run, or a def that reads its own
//! name, splices into its body.

use super::scan::{form_reads, value_reads, Read};
use crate::epoch::placement::{is_begin, Def};
use crate::syntax::Syntax;

/// The path to each begin that stands directly in a body and holds a form of
/// a run, or a def whose value reads its own name directly, in the order of
/// the walk.
pub(super) fn splices(forms: &[Syntax]) -> Vec<Vec<usize>> {
    let mut paths: Vec<Vec<usize>> = Vec::new();
    super::bodies::each(forms, &mut |path, items, start| {
        let Some(reading) = Reading::of(items, start) else {
            return;
        };
        for (k, slot) in reading.slots.iter().enumerate() {
            let in_run = reading.runs.iter().any(|(a, b)| (*a..=*b).contains(&k));
            let form = &reading.read[k];
            let own = form
                .def
                .is_some_and(|def| value_reads(def, &form.names).iter().any(|r| !r.in_lambda));
            let begin = [path, &[slot.index]].concat();
            if !slot.direct && (in_run || own) && !paths.contains(&begin) {
                paths.push(begin);
            }
        }
    });
    paths
}

/// One form of a body, through every `begin` in it.
pub(super) struct Slot<'t> {
    pub node: &'t Syntax,
    /// Its index among the forms of the body's parent.
    pub index: usize,
    /// Whether it stands there itself, not inside a `begin`.
    pub direct: bool,
}

/// A form of a body read as a def: the def, and the names it binds.
pub(super) struct Form<'t> {
    pub def: Option<Def<'t>>,
    pub names: Vec<&'t str>,
}

/// One body's forms, the forward reads in them, and the runs those make.
pub(super) struct Reading<'t> {
    pub slots: Vec<Slot<'t>>,
    pub read: Vec<Form<'t>>,
    /// Each forward read inside a lambda: the form that reads, the form of
    /// the name's first def, and the read.
    pub forward: Vec<(usize, usize, Read<'t>)>,
    pub runs: Vec<(usize, usize)>,
}

impl<'t> Reading<'t> {
    /// Read the forms of `items` from `start` on, a body.
    pub(super) fn of(items: &'t [Syntax], start: usize) -> Option<Reading<'t>> {
        let slots = slots(items.get(start..)?, start);
        let read: Vec<Form<'t>> = slots
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
        let mut forward: Vec<(usize, usize, Read<'t>)> = Vec::new();
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
        Some(Reading {
            slots,
            read,
            forward,
            runs,
        })
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
