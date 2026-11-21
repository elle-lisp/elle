// audited: 2026-11-20
//! The walk behind the epoch 17 rule: the bodies it is inside, and the form it is in, directly in each.
//!
//! docs/epochs/bodies.md
//!
//! A form stands directly in a body, or it does not. Each run of defs that
//! does not stands in a sequence: the forms of a `when` after its test, of a
//! `begin` outside a body, or one operand alone. [`forms`] says how each form
//! places its forms, and [`run`] what each run of defs in a sequence does.
//! The walk runs twice: the first time it places no let, and [`assign`] says
//! which defs in a sequence assign instead.

use super::{is_begin, Assigns, Def, Plan};
use crate::syntax::{Syntax, SyntaxKind};
use forms::pattern_binds;

mod assign;
mod forms;
mod run;

/// A body the walk is inside: a function, a `let` form, a `block`, a match
/// arm, the file or a template. Its forms run through every `begin` in it,
/// and `at` is the one the walk is in.
struct Body<'t> {
    forms: Vec<&'t Syntax>,
    at: usize,
    next: usize,
    /// The path to the form `at` names.
    home: Vec<usize>,
    /// The names the form that opens the body binds over it.
    binds: Vec<&'t str>,
    /// The names the first walk binds in the body, for a def that assigns.
    decls: Vec<&'t str>,
    /// How many forms around the walk, inside the body, keep a def inside
    /// them up to epoch 16 ([`Walker::kept`]).
    kept: usize,
}

pub(super) struct Walker<'t> {
    plan: Plan,
    bodies: Vec<Body<'t>>,
    path: Vec<usize>,
    /// The depth of `bodies` at each template the walk is inside.
    templates: Vec<usize>,
    /// In the first walk, the defs that assign. The walk then places no let.
    assigns: Option<Assigns>,
}

impl<'t> Walker<'t> {
    /// Walk a file's forms, a body, and answer the plan.
    pub(super) fn over(forms: &'t [Syntax]) -> Plan {
        Walker::walked(forms, None).plan
    }

    /// Walk a file's forms, a body, and answer the defs that assign.
    pub(super) fn assigns(forms: &'t [Syntax]) -> Assigns {
        let assigns = Assigns {
            defs: Vec::new(),
            decls: Vec::new(),
        };
        let walker = Walker::walked(forms, Some(assigns));
        walker.assigns.expect("the first walk keeps its assigns")
    }

    fn walked(forms: &'t [Syntax], assigns: Option<Assigns>) -> Walker<'t> {
        let mut walker = Walker {
            plan: Plan {
                placements: Vec::new(),
                notes: Vec::new(),
            },
            bodies: Vec::new(),
            path: Vec::new(),
            templates: Vec::new(),
            assigns,
        };
        walker.body(forms, 0, Vec::new());
        walker
    }

    /// Walk the child `i` of the current form with `f`.
    fn at(&mut self, i: usize, f: impl FnOnce(&mut Self)) {
        self.path.push(i);
        f(self);
        self.path.pop();
    }

    /// Walk `items` from `from` on as a body of their own, over which the
    /// form that opens it binds `binds`.
    fn body(&mut self, items: &'t [Syntax], from: usize, binds: Vec<&'t str>) {
        self.body_of(items, from..items.len(), binds);
    }

    /// Walk the `range` of `items` as a body of its own.
    fn body_of(&mut self, items: &'t [Syntax], range: std::ops::Range<usize>, binds: Vec<&'t str>) {
        self.bodies.push(Body {
            forms: flatten(&items[range.clone()]),
            at: 0,
            next: 0,
            home: Vec::new(),
            binds,
            decls: Vec::new(),
            kept: 0,
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
            body.home = self.path.clone();
        }
        match Def::of(item) {
            Some(def) => self.def_parts(def),
            None => self.form(item),
        }
    }

    /// Walk the value of a def, or the body of a `defn`.
    fn def_parts(&mut self, def: Def<'t>) {
        match def.kind {
            super::Kind::Defn => self.body(def.items, 3, pattern_binds(def.items, 2)),
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
                self.body(children, 0, Vec::new());
                self.templates.pop();
            }
            SyntaxKind::Unquote(_) | SyntaxKind::UnquoteSplicing(_) => {
                // An unquoted form runs when the macro does, in the bodies
                // outside the template.
                let outer = self.templates.pop();
                let inner = outer.map(|depth| self.bodies.split_off(depth));
                self.kept(|w| w.operands(node, 0));
                if let (Some(depth), Some(inner)) = (outer, inner) {
                    self.bodies.extend(inner);
                    self.templates.push(depth);
                }
            }
            SyntaxKind::List(items) => self.list(node, items),
            _ => self.unknown(node),
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
