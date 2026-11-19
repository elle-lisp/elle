// audited: 2026-11-19
//! Where a form reads a name, as the epoch 18 rule asks it: directly, or inside a lambda, under the scope epoch 17 gave.
//!
//! docs/epochs.md
//! docs/bindings.md
//!
//! Up to epoch 17 a body binds every name its defs bind before any form runs.
//! In a body the scan enters, a direct read before the body's first def of a
//! name reads past it, and a read inside a lambda finds the body's own
//! binding. So the scan tracks two sets of names: the ones a read at this
//! point finds, and the ones a read inside a lambda opened here finds.

use crate::epoch::placement::reads::{bound, each_parts, names_read, without};
use crate::epoch::placement::{is_begin, Def, Kind};
use crate::syntax::{Span, Syntax, SyntaxKind};

/// A read of a name the scan looks for.
pub(super) struct Read<'t> {
    pub name: &'t str,
    pub at: Span,
    /// Whether the read stands inside a lambda the scanned form holds.
    pub in_lambda: bool,
}

/// The reads of `names` in `node`, a form that stands directly in the body
/// the rule reads. A def's own name and target are not reads; its value is.
pub(super) fn form_reads<'t>(node: &'t Syntax, names: &[&'t str]) -> Vec<Read<'t>> {
    match Def::of(node) {
        Some(def) => value_reads(def, names),
        None => {
            let mut scan = Scan::default();
            scan.node(node, &Live::of(names));
            scan.reads
        }
    }
}

/// The reads of `names` in the value of `def`. A `defn`'s value is the
/// function its parameters and body make.
pub(super) fn value_reads<'t>(def: Def<'t>, names: &[&'t str]) -> Vec<Read<'t>> {
    let mut scan = Scan::default();
    let live = Live::of(names);
    match def.kind {
        Kind::Defn => scan.lambda(&def.items[2], &def.items[3..], &live),
        Kind::Def | Kind::Var => scan.node(&def.items[2], &live),
    }
    scan.reads
}

/// What a read at one point of the scan finds.
#[derive(Clone)]
struct Live<'t> {
    /// The names a read here finds.
    here: Vec<&'t str>,
    /// The names a read inside a lambda opened here finds.
    in_lambda: Vec<&'t str>,
    /// Whether this point stands inside a lambda.
    lambda: bool,
}

impl<'t> Live<'t> {
    fn of(names: &[&'t str]) -> Self {
        Live {
            here: names.to_vec(),
            in_lambda: names.to_vec(),
            lambda: false,
        }
    }

    fn none(&self) -> bool {
        self.here.is_empty() && self.in_lambda.is_empty()
    }

    /// The same point with `hidden` bound again.
    fn without(&self, hidden: &[&str]) -> Self {
        Live {
            here: without(&self.here, hidden),
            in_lambda: without(&self.in_lambda, hidden),
            lambda: self.lambda,
        }
    }
}

#[derive(Default)]
struct Scan<'t> {
    reads: Vec<Read<'t>>,
}

impl<'t> Scan<'t> {
    fn node(&mut self, node: &'t Syntax, live: &Live<'t>) {
        if live.none() {
            return;
        }
        match &node.kind {
            SyntaxKind::Symbol(s) => {
                if let Some(name) = live.here.iter().find(|n| names_read(s.as_str(), n)) {
                    self.reads.push(Read {
                        name,
                        at: node.span,
                        in_lambda: live.lambda,
                    });
                }
            }
            SyntaxKind::Quote(_) => {}
            // A template is data, but for the forms it unquotes.
            SyntaxKind::Quasiquote(_) => self.template(node, live),
            SyntaxKind::List(items) => self.form(items, live),
            _ => self.all(node.kind.children(), live),
        }
    }

    fn all(&mut self, nodes: &'t [Syntax], live: &Live<'t>) {
        for node in nodes {
            self.node(node, live);
        }
    }

    fn template(&mut self, node: &'t Syntax, live: &Live<'t>) {
        for child in node.kind.children() {
            match child.kind {
                SyntaxKind::Unquote(_) | SyntaxKind::UnquoteSplicing(_) => {
                    self.all(child.kind.children(), live)
                }
                _ => self.template(child, live),
            }
        }
    }

    fn form(&mut self, items: &'t [Syntax], live: &Live<'t>) {
        match items.first().and_then(Syntax::as_symbol) {
            Some("quote") => {}
            // A macro's body runs when the macro expands, outside this body.
            Some("defmacro") => {}
            Some("fn") if items.len() >= 2 => self.lambda(&items[1], &items[2..], live),
            Some("defn") if items.len() >= 3 => self.lambda(&items[2], &items[3..], live),
            Some("def" | "var") if items.len() == 3 => self.node(&items[2], live),
            Some(head @ ("let" | "let*" | "letrec")) if items.len() >= 2 => {
                self.bindings(head == "letrec", items, live)
            }
            Some("each") => match each_parts(items) {
                Some((var, coll, body)) => {
                    self.node(&items[coll], live);
                    self.body(body, &live.without(&bound(&items[var]).unwrap_or_default()));
                }
                None => self.all(items, live),
            },
            Some("block") => self.body(&items[1..], live),
            Some("parameterize") if items.len() >= 2 => {
                self.node(&items[1], live);
                self.body(&items[2..], live);
            }
            Some("match") if items.len() >= 2 => self.arms(items, live),
            _ => self.all(items, live),
        }
    }

    /// A function: its parameters bind their names again, and every read in
    /// its body stands inside a lambda.
    fn lambda(&mut self, params: &'t Syntax, body: &'t [Syntax], live: &Live<'t>) {
        let hidden = bound(params).unwrap_or_default();
        let names = without(&live.in_lambda, &hidden);
        let inner = Live {
            here: names.clone(),
            in_lambda: names,
            lambda: true,
        };
        self.body(body, &inner);
    }

    /// A `let`, `let*` or `letrec` form. Each pair's value reads past the
    /// names its own and later pairs bind, but a `letrec` binds every name
    /// before any value.
    fn bindings(&mut self, letrec: bool, items: &'t [Syntax], live: &Live<'t>) {
        let Some(pairs) = items[1].as_list_or_tuple() else {
            return self.all(items, live);
        };
        let mut live = live.clone();
        if letrec {
            for pair in pairs.chunks(2) {
                live = live.without(&bound(&pair[0]).unwrap_or_default());
            }
        }
        for pair in pairs.chunks(2) {
            if let [_, value] = pair {
                self.node(value, &live);
            }
            live = live.without(&bound(&pair[0]).unwrap_or_default());
        }
        self.body(&items[2..], &live);
    }

    /// A `match`: the value and each guard are operands, and each arm is a
    /// body of one form, where the names its pattern binds hide the outer ones.
    fn arms(&mut self, items: &'t [Syntax], live: &Live<'t>) {
        self.node(&items[1], live);
        let mut i = 2;
        while i < items.len() {
            if i + 1 == items.len() {
                self.node(&items[i], live);
                break;
            }
            let arm = live.without(&bound(&items[i]).unwrap_or_default());
            let guarded = i + 3 < items.len() && items[i + 1].is_symbol("when");
            let body = if guarded {
                self.node(&items[i + 2], &arm);
                i + 3
            } else {
                i + 1
            };
            self.body(&items[body..=body], &arm);
            i = body + 1;
        }
    }

    /// A body: a name one of its defs binds is that def's for a read inside a
    /// lambda, and for a direct read from the def on.
    fn body(&mut self, forms: &'t [Syntax], live: &Live<'t>) {
        let flat = flatten(forms);
        let all: Vec<&str> = flat
            .iter()
            .filter_map(|f| Def::of(f).and_then(|d| d.names().ok()))
            .flatten()
            .collect();
        let in_lambda = without(&live.in_lambda, &all);
        let mut seen: Vec<&str> = Vec::new();
        for form in flat {
            let here = Live {
                here: without(&live.here, &seen),
                in_lambda: in_lambda.clone(),
                lambda: live.lambda,
            };
            match Def::of(form) {
                Some(def) => {
                    match def.kind {
                        Kind::Defn => self.lambda(&def.items[2], &def.items[3..], &here),
                        Kind::Def | Kind::Var => self.node(&def.items[2], &here),
                    }
                    seen.extend(def.names().unwrap_or_default());
                }
                None => self.node(form, &here),
            }
        }
    }
}

/// The forms of a body, through every `begin` in it.
fn flatten(forms: &[Syntax]) -> Vec<&Syntax> {
    let mut out = Vec::new();
    for form in forms {
        if is_begin(form) {
            out.extend(flatten(&form.kind.children()[1..]));
        } else {
            out.push(form);
        }
    }
    out
}
