// audited: 2026-11-19
//! The epoch 18 rule's moves in one body: each def a forward read reaches moves above the read, where no code that runs can tell.
//!
//! docs/epochs.md
//!
//! A form is quiet when evaluating it runs no code: a def whose value is a
//! function, a literal, a quoted form or a name, and a `defmacro`. A def may
//! move when it is quiet, binds one name the body defines once, is no `var`,
//! and is not the body's last form. Defs that read each other are one unit,
//! a strongly connected component of the reads, and move together. [`place`]
//! says where each unit lands.

use super::scan::{form_reads, Read};
use super::Reorder;
use crate::epoch::placement::{is_begin, Def, Kind};
use crate::syntax::{Syntax, SyntaxKind};

mod place;

/// What the moves read of one form of a body.
struct Facts<'t> {
    /// The names the form binds: a def's, a `defmacro`'s, or those of the
    /// defs in a `begin`.
    names: Vec<&'t str>,
    /// Whether evaluating the form runs no code.
    quiet: bool,
    /// Whether the form is a def of a shape that may move.
    shaped: bool,
    /// Its reads of the names the body binds.
    reads: Vec<Read<'t>>,
}

/// One body, as the moves read it.
struct Body<'t> {
    facts: Vec<Facts<'t>>,
    /// The unit each form belongs to.
    unit: Vec<usize>,
    /// The forms of each unit, in order.
    members: Vec<Vec<usize>>,
    /// Whether each form may move.
    movable: Vec<bool>,
}

/// The new order of the body whose forms are `items` from `start` on, held
/// by the form at `path`, or `None` when no def moves.
pub(super) fn of(path: &[usize], items: &[Syntax], start: usize) -> Option<Reorder> {
    let forms = items.get(start..)?;
    if forms.len() < 2 {
        return None;
    }
    let body = Body::read(forms);
    let (order, pulled) = place::order(&body);
    let moved = moved(&order, &pulled);
    if moved.is_empty() {
        return None;
    }
    Some(Reorder {
        path: path.to_vec(),
        start,
        order: order.iter().map(|k| start + k).collect(),
        moved: moved.iter().map(|k| start + k).collect(),
    })
}

impl<'t> Facts<'t> {
    fn of(node: &'t Syntax) -> Self {
        let mut facts = Facts {
            names: Vec::new(),
            quiet: false,
            shaped: false,
            reads: Vec::new(),
        };
        if let Some(def) = Def::of(node) {
            facts.names = def.names().unwrap_or_default();
            facts.quiet = def.kind == Kind::Defn || quiet_value(&def.items[2]);
            facts.shaped = facts.quiet
                && def.kind != Kind::Var
                && facts.names.len() == 1
                && def.name().as_symbol().is_some_and(|n| !n.starts_with('@'));
        } else if is_begin(node) {
            facts.names = begin_names(node);
        } else if let Some(items) = node.as_list() {
            if items.first().is_some_and(|h| h.is_symbol("defmacro")) {
                facts.names = items
                    .get(1)
                    .and_then(Syntax::as_symbol)
                    .into_iter()
                    .collect();
                facts.quiet = true;
            }
        }
        facts
    }
}

impl<'t> Body<'t> {
    fn read(forms: &'t [Syntax]) -> Self {
        let mut facts: Vec<Facts<'t>> = forms.iter().map(Facts::of).collect();
        let mut all: Vec<&'t str> = Vec::new();
        for name in facts.iter().flat_map(|f| f.names.iter()) {
            if !all.contains(name) {
                all.push(name);
            }
        }
        for (form, facts) in forms.iter().zip(facts.iter_mut()) {
            facts.reads = form_reads(form, &all);
        }
        let mut body = Body {
            facts,
            unit: Vec::new(),
            members: Vec::new(),
            movable: Vec::new(),
        };
        let n = forms.len();
        body.movable = (0..n)
            .map(|k| {
                let f = &body.facts[k];
                f.shaped && k + 1 < n && body.defs_of(f.names[0]).count() == 1
            })
            .collect();
        let edges: Vec<Vec<usize>> = (0..n)
            .map(|k| {
                let f = &body.facts[k];
                f.reads
                    .iter()
                    .filter(|r| !f.names.contains(&r.name))
                    .flat_map(|r| body.defs_of(r.name))
                    .filter(|&d| d != k)
                    .collect()
            })
            .collect();
        body.unit = components(&edges);
        body.members = vec![Vec::new(); body.unit.iter().max().map_or(0, |m| m + 1)];
        for (k, &u) in body.unit.iter().enumerate() {
            body.members[u].push(k);
        }
        body
    }

    /// The forms that bind `name`.
    fn defs_of<'a>(&'a self, name: &'a str) -> impl Iterator<Item = usize> + 'a {
        (0..self.facts.len()).filter(move |&k| self.facts[k].names.contains(&name))
    }

    /// The form that first binds `name`.
    fn first_def(&self, name: &str) -> Option<usize> {
        self.defs_of(name).next()
    }
}

/// Whether evaluating `node` as a def's value runs no code.
fn quiet_value(node: &Syntax) -> bool {
    match &node.kind {
        SyntaxKind::Nil
        | SyntaxKind::Bool(_)
        | SyntaxKind::Int(_)
        | SyntaxKind::Float(_)
        | SyntaxKind::Keyword(_)
        | SyntaxKind::String(_)
        | SyntaxKind::StringMut(_)
        | SyntaxKind::Quote(_) => true,
        // A qualified symbol is a call of `get`.
        SyntaxKind::Symbol(_) => node
            .as_symbol()
            .is_some_and(|s| !s.get(1..).unwrap_or_default().contains(':')),
        SyntaxKind::List(items) => items
            .first()
            .is_some_and(|h| h.is_symbol("fn") || h.is_symbol("quote")),
        _ => false,
    }
}

/// The names the defs in a `begin` bind, through every `begin` in it.
fn begin_names(node: &Syntax) -> Vec<&str> {
    let mut names = Vec::new();
    for child in &node.kind.children()[1..] {
        if is_begin(child) {
            names.extend(begin_names(child));
        } else if let Some(def) = Def::of(child) {
            names.extend(def.names().unwrap_or_default());
        }
    }
    names
}

/// The forms of `order` that moved: those whose order to the forms that
/// stayed changed. Every form no read pulled stays, and a pulled form that
/// stands between the forms that stay as it stood before stays too.
fn moved(order: &[usize], pulled: &[bool]) -> Vec<usize> {
    let mut moved = Vec::new();
    let mut last: Option<usize> = None;
    for (at, &form) in order.iter().enumerate() {
        let next = order[at + 1..].iter().find(|&&g| !pulled[g]);
        let stays =
            !pulled[form] || (last.is_none_or(|l| l < form) && next.is_none_or(|&g| form < g));
        if stays {
            last = Some(form);
        } else {
            moved.push(form);
        }
    }
    moved
}

/// The strongly connected component of each node of the graph `edges`.
fn components(edges: &[Vec<usize>]) -> Vec<usize> {
    struct Tarjan<'e> {
        edges: &'e [Vec<usize>],
        index: Vec<Option<usize>>,
        low: Vec<usize>,
        stack: Vec<usize>,
        on_stack: Vec<bool>,
        next: usize,
        component: Vec<usize>,
        count: usize,
    }
    impl Tarjan<'_> {
        fn visit(&mut self, v: usize) {
            self.index[v] = Some(self.next);
            self.low[v] = self.next;
            self.next += 1;
            self.stack.push(v);
            self.on_stack[v] = true;
            for &w in &self.edges[v] {
                match self.index[w] {
                    None => {
                        self.visit(w);
                        self.low[v] = self.low[v].min(self.low[w]);
                    }
                    Some(i) if self.on_stack[w] => self.low[v] = self.low[v].min(i),
                    Some(_) => {}
                }
            }
            if Some(self.low[v]) == self.index[v] {
                while let Some(w) = self.stack.pop() {
                    self.on_stack[w] = false;
                    self.component[w] = self.count;
                    if w == v {
                        break;
                    }
                }
                self.count += 1;
            }
        }
    }
    let n = edges.len();
    let mut t = Tarjan {
        edges,
        index: vec![None; n],
        low: vec![0; n],
        stack: Vec::new(),
        on_stack: vec![false; n],
        next: 0,
        component: vec![0; n],
        count: 0,
    };
    for v in 0..n {
        if t.index[v].is_none() {
            t.visit(v);
        }
    }
    t.component
}
