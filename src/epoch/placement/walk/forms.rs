// audited: 2026-11-20
//! How each form the epoch 17 rule knows places its forms: in a body, as a sequence, or kept inside the form.
//!
//! docs/epochs/bodies.md
//!
//! A form the rule knows opens a body, or walks its operands as sequences. A
//! form that keeps a def inside it up to epoch 16 walks them through
//! [`Walker::kept`], and a form the rule does not know is one of those.

use super::super::reads::{bound, each_parts};
use super::super::Def;
use super::Walker;
use crate::syntax::Syntax;

impl<'t> Walker<'t> {
    /// Walk a list by its head: each form the rule knows places its forms.
    pub(super) fn list(&mut self, node: &'t Syntax, items: &'t [Syntax]) {
        let n = items.len();
        match items.first().and_then(Syntax::as_symbol) {
            Some("quote") => {}
            Some("fn") => self.body(items, 2, pattern_binds(items, 1)),
            Some("defmacro") => self.body(items, 3, pattern_binds(items, 2)),
            Some("let" | "letrec" | "let*") if n >= 2 => {
                self.kept(|w| {
                    w.at(1, |w| {
                        w.pairs(&items[1], |w, vector, i| {
                            w.sequence(vector, i + 1, i + 2, false)
                        })
                    })
                });
                self.body(items, 2, binds(&items[1]));
            }
            Some("parameterize") if n >= 2 => {
                self.at(1, |w| {
                    for (i, pair) in items[1].kind.children().iter().enumerate() {
                        w.at(i, |w| w.operands(pair, 0));
                    }
                });
                self.body(items, 2, Vec::new());
            }
            Some("block") => self.body(items, 1, Vec::new()),
            Some("each") if each_parts(items).is_some() => {
                let (var, coll, _) = each_parts(items).expect("checked");
                self.kept(|w| w.sequence(node, coll, coll + 1, false));
                self.body(items, coll + 1, pattern_binds(items, var));
            }
            Some("match") if n >= 2 => self.arms(node, items),
            Some("while" | "when" | "unless") if n >= 2 => self.test_and_body(node, n),
            Some("repeat") if n >= 2 => self.kept(|w| w.test_and_body(node, n)),
            Some("forever") => self.sequence(node, 1, n, false),
            Some("begin" | "do") => self.sequence(node, 1, n, true),
            Some("if" | "cond" | "and" | "or" | "assign" | "def" | "var") => self.operands(node, 1),
            _ => self.unknown(node),
        }
    }

    /// Walk the first operand of `node` and then the rest, up to `n`, each as
    /// a sequence.
    fn test_and_body(&mut self, node: &'t Syntax, n: usize) {
        self.sequence(node, 1, 2, false);
        self.sequence(node, 2, n, false);
    }

    /// Walk with `f` inside a form that keeps a def inside it up to epoch 16,
    /// so no def there assigns: a `let` binding's value, an `each`
    /// collection, `repeat`, a `match` guard, an unquote, or a form the rule
    /// does not know.
    pub(super) fn kept(&mut self, f: impl FnOnce(&mut Self)) {
        self.bodies.last_mut().expect("the walk is in a body").kept += 1;
        f(self);
        self.bodies.last_mut().expect("the walk is in a body").kept -= 1;
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
                self.kept(|w| w.sequence(node, i + 2, i + 3, false));
                i + 3
            } else {
                i + 1
            };
            self.body_of(items, arm..arm + 1, pattern_binds(items, i));
            i = arm + 1;
        }
    }

    /// Walk each child of `node` from `from` on as an operand of its own.
    pub(super) fn operands(&mut self, node: &'t Syntax, from: usize) {
        for i in from..node.kind.children().len() {
            self.sequence(node, i, i + 1, false);
        }
    }

    /// Walk the children of a form the rule does not know. A def directly
    /// there stays, with a note naming the form.
    pub(super) fn unknown(&mut self, node: &'t Syntax) {
        self.kept(|w| {
            for (i, child) in node.kind.children().iter().enumerate() {
                match Def::of(child) {
                    Some(def) => {
                        let place = describe(node);
                        w.note(
                            def,
                            format!("it stands in {place}, which the rule does not read"),
                        );
                        w.at(i, |w| w.def_parts(def));
                    }
                    None => w.at(i, |w| w.form(child)),
                }
            }
        });
    }
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

/// The names the pattern `items[i]` binds, or none when there is no such
/// pattern.
pub(super) fn pattern_binds(items: &[Syntax], i: usize) -> Vec<&str> {
    items.get(i).and_then(bound).unwrap_or_default()
}

/// The names the patterns of a `let` form's vector bind.
fn binds(vector: &Syntax) -> Vec<&str> {
    let pairs = vector.as_list_or_tuple().unwrap_or_default();
    pairs
        .chunks(2)
        .flat_map(|pair| bound(&pair[0]).unwrap_or_default())
        .collect()
}
