// audited: 2026-11-17
//! The walk behind the epoch 17 rule: where each form the rule knows places its forms.
//!
//! docs/epochs.md
//!
//! A form stands directly in a body, or it does not. Each run of defs that
//! does not stands in a sequence: the forms of a `when` after its test, of a
//! `begin` outside a body, or one operand alone. [`run`] says what each run
//! of defs in a sequence does.

use super::reads::each_parts;
use super::{is_begin, Def, Plan};
use crate::syntax::{Syntax, SyntaxKind};

mod run;

/// A body the walk is inside: a function, a `let` form, a `block`, a match
/// arm, the file or a template. Its forms run through every `begin` in it,
/// and `at` is the one the walk is in.
struct Body<'t> {
    forms: Vec<&'t Syntax>,
    at: usize,
    next: usize,
}

pub(super) struct Walker<'t> {
    plan: Plan,
    bodies: Vec<Body<'t>>,
    path: Vec<usize>,
    /// The depth of `bodies` at each template the walk is inside.
    templates: Vec<usize>,
}

impl<'t> Walker<'t> {
    /// Walk a file's forms, a body, and answer the plan.
    pub(super) fn over(forms: &'t [Syntax]) -> Plan {
        let mut walker = Walker {
            plan: Plan {
                placements: Vec::new(),
                notes: Vec::new(),
            },
            bodies: Vec::new(),
            path: Vec::new(),
            templates: Vec::new(),
        };
        walker.body(forms, 0);
        walker.plan
    }

    /// Walk the child `i` of the current form with `f`.
    fn at(&mut self, i: usize, f: impl FnOnce(&mut Self)) {
        self.path.push(i);
        f(self);
        self.path.pop();
    }

    /// Walk `items` from `from` on as a body of their own.
    fn body(&mut self, items: &'t [Syntax], from: usize) {
        self.body_of(items, from..items.len());
    }

    /// Walk the `range` of `items` as a body of its own.
    fn body_of(&mut self, items: &'t [Syntax], range: std::ops::Range<usize>) {
        self.bodies.push(Body {
            forms: flatten(&items[range.clone()]),
            at: 0,
            next: 0,
        });
        for i in range {
            self.at(i, |w| w.body_form(&items[i], true));
        }
        self.bodies.pop();
    }

    /// Walk `item`, which stands directly in a body. `counted` is false in the
    /// body of a let the rule makes, which the walk's bodies do not hold.
    fn body_form(&mut self, item: &'t Syntax, counted: bool) {
        if is_begin(item) {
            for (i, child) in item.kind.children().iter().enumerate().skip(1) {
                self.at(i, |w| w.body_form(child, counted));
            }
            return;
        }
        if counted {
            let body = self
                .bodies
                .last_mut()
                .expect("a body form stands in a body");
            body.at = body.next;
            body.next += 1;
        }
        match Def::of(item) {
            Some(def) => self.def_parts(def),
            None => self.form(item),
        }
    }

    /// Walk the value of a def, or the body of a `defn`.
    fn def_parts(&mut self, def: Def<'t>) {
        match def.kind {
            super::Kind::Defn => self.body(def.items, 3),
            _ => self.sequence(def.node, 2, 3, false),
        }
    }

    /// Walk `node`, which stands anywhere but directly in a body.
    fn form(&mut self, node: &'t Syntax) {
        let children = node.kind.children();
        match &node.kind {
            SyntaxKind::Quote(_) => {}
            SyntaxKind::Quasiquote(_) => {
                // The top of a template may land in a body, so it is read as one.
                self.templates.push(self.bodies.len());
                self.body(children, 0);
                self.templates.pop();
            }
            SyntaxKind::Unquote(_) | SyntaxKind::UnquoteSplicing(_) => {
                // An unquoted form runs when the macro does, in the bodies
                // outside the template.
                let outer = self.templates.pop();
                let inner = outer.map(|depth| self.bodies.split_off(depth));
                self.operands(node, 0);
                if let (Some(depth), Some(inner)) = (outer, inner) {
                    self.bodies.extend(inner);
                    self.templates.push(depth);
                }
            }
            SyntaxKind::List(items) => self.list(node, items),
            _ => self.unknown(node),
        }
    }

    /// Walk a list by its head: each form the rule knows places its forms.
    fn list(&mut self, node: &'t Syntax, items: &'t [Syntax]) {
        let n = items.len();
        match items.first().and_then(Syntax::as_symbol) {
            Some("quote") => {}
            Some("fn") => self.body(items, 2),
            Some("defmacro") => self.body(items, 3),
            Some("let" | "letrec" | "let*") if n >= 2 => {
                self.at(1, |w| {
                    w.pairs(&items[1], |w, vector, i| {
                        w.sequence(vector, i + 1, i + 2, false)
                    })
                });
                self.body(items, 2);
            }
            Some("parameterize") if n >= 2 => {
                self.at(1, |w| {
                    for (i, pair) in items[1].kind.children().iter().enumerate() {
                        w.at(i, |w| w.operands(pair, 0));
                    }
                });
                self.body(items, 2);
            }
            Some("block") => self.body(items, 1),
            Some("each") if each_parts(items).is_some() => {
                let (_, coll, _) = each_parts(items).expect("checked");
                self.sequence(node, coll, coll + 1, false);
                self.body(items, coll + 1);
            }
            Some("match") if n >= 2 => self.arms(node, items),
            Some("while" | "when" | "unless" | "repeat") if n >= 2 => {
                self.sequence(node, 1, 2, false);
                self.sequence(node, 2, n, false);
            }
            Some("forever") => self.sequence(node, 1, n, false),
            Some("begin" | "do") => self.sequence(node, 1, n, true),
            Some("if" | "cond" | "and" | "or" | "assign" | "def" | "var") => self.operands(node, 1),
            _ => self.unknown(node),
        }
    }

    /// Walk each binding pair of a `let` form's vector with `f`, which takes
    /// the vector and the pattern's index.
    fn pairs(&mut self, vector: &'t Syntax, f: impl Fn(&mut Self, &'t Syntax, usize)) {
        let Some(items) = vector.as_list_or_tuple() else {
            return self.form(vector);
        };
        for i in (0..items.len().saturating_sub(1)).step_by(2) {
            f(self, vector, i);
        }
    }

    /// Walk a `match`: the value and each guard are operands, and each arm is a
    /// body of one form.
    fn arms(&mut self, node: &'t Syntax, items: &'t [Syntax]) {
        self.sequence(node, 1, 2, false);
        let mut i = 2;
        while i + 1 < items.len() {
            let arm = if i + 3 < items.len() && items[i + 1].is_symbol("when") {
                self.sequence(node, i + 2, i + 3, false);
                i + 3
            } else {
                i + 1
            };
            self.body_of(items, arm..arm + 1);
            i = arm + 1;
        }
    }

    /// Walk each child of `node` from `from` on as an operand of its own.
    fn operands(&mut self, node: &'t Syntax, from: usize) {
        for i in from..node.kind.children().len() {
            self.sequence(node, i, i + 1, false);
        }
    }

    /// Walk the children of a form the rule does not know. A def directly
    /// there stays, with a note naming the form.
    fn unknown(&mut self, node: &'t Syntax) {
        for (i, child) in node.kind.children().iter().enumerate() {
            match Def::of(child) {
                Some(def) => {
                    let place = describe(node);
                    self.note(
                        def,
                        format!("it stands in {place}, which the rule does not read"),
                    );
                    self.at(i, |w| w.def_parts(def));
                }
                None => self.at(i, |w| w.form(child)),
            }
        }
    }
}

/// The forms of a body, through every `begin` in it.
fn flatten(items: &[Syntax]) -> Vec<&Syntax> {
    let mut forms = Vec::new();
    for item in items {
        if is_begin(item) {
            forms.extend(flatten(&item.kind.children()[1..]));
        } else {
            forms.push(item);
        }
    }
    forms
}

/// The form a note names: `the (protect ...) form`, `a call`, `the array`.
fn describe(node: &Syntax) -> String {
    match node.as_list().and_then(<[Syntax]>::first) {
        Some(head) => match head.as_symbol() {
            Some(name) => format!("the ({name} ...) form"),
            None => "a call".to_string(),
        },
        None => format!("the {}", node.kind_label()),
    }
}
