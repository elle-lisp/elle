// audited: 2026-11-20
//! Whether a form reads a name, as the epoch 17 and 18 rules ask it: a binding form that binds the name again hides it.
//!
//! docs/epochs/bodies.md
//!
//! The answer may err toward a read and never away from one. A read the scan
//! invents only leaves a def where it was. So the scan knows the forms that
//! bind a name again: `fn`, the `let` forms, `each`, the prelude's binding
//! macros ([`prelude`]), and a `def` from that def on in the body or sequence
//! that holds it. It counts every other occurrence of a name, a `match`
//! pattern included.

use super::{is_begin, Def};
use crate::syntax::{Syntax, SyntaxKind};

mod prelude;

/// A run's forms in their parent: the children of `parent` from `first` up to
/// `end`, which a scan for reads outside the run passes over.
#[derive(Clone, Copy)]
pub(super) struct Region<'t> {
    pub parent: &'t Syntax,
    pub first: usize,
    pub end: usize,
}

/// A scan for reads, passing over one region or none.
#[derive(Clone, Copy)]
pub(super) struct Scan<'t> {
    pub skip: Option<Region<'t>>,
}

impl Scan<'static> {
    /// The scan that passes over nothing.
    pub(super) const PLAIN: Scan<'static> = Scan { skip: None };
}

impl Scan<'_> {
    /// Whether `node` reads one of `names`.
    pub(super) fn reads(&self, node: &Syntax, names: &[&str]) -> bool {
        if names.is_empty() {
            return false;
        }
        let children = node.kind.children();
        if let Some(region) = self.skip.filter(|r| std::ptr::eq(r.parent, node)) {
            return children
                .iter()
                .enumerate()
                .any(|(i, c)| !(region.first..region.end).contains(&i) && self.reads(c, names));
        }
        match &node.kind {
            SyntaxKind::Symbol(s) => names.iter().any(|n| names_read(s.as_str(), n)),
            SyntaxKind::Quote(_) => false,
            SyntaxKind::List(items) => self.form(items, names),
            _ => self.any(children, names),
        }
    }

    /// Whether a function with `params` and `body` reads one of `names`.
    pub(super) fn function(&self, params: &Syntax, body: &[Syntax], names: &[&str]) -> bool {
        self.body(body, &hidden(names, params))
    }

    fn any(&self, nodes: &[Syntax], names: &[&str]) -> bool {
        nodes.iter().any(|n| self.reads(n, names))
    }

    /// A body: a name one of its own defs binds is that def's from the def on.
    fn body(&self, forms: &[Syntax], names: &[&str]) -> bool {
        self.in_order(forms, &mut names.to_vec())
    }

    /// Whether `forms`, in order and through every `begin`, read one of
    /// `live`. A direct read before a def reads past it, so the def's names
    /// leave `live` only after its value. A function value reads the def's own
    /// binding, so its names leave first. A `begin` that holds the region the
    /// scan passes over is read whole, because the let that takes its run
    /// ends with it.
    pub(super) fn in_order<'s>(
        &self,
        forms: impl IntoIterator<Item = &'s Syntax>,
        live: &mut Vec<&str>,
    ) -> bool {
        for form in forms {
            if live.is_empty() {
                return false;
            }
            let holds_region = self.skip.is_some_and(|r| std::ptr::eq(r.parent, form));
            if is_begin(form) && !holds_region {
                if self.in_order(&form.kind.children()[1..], live) {
                    return true;
                }
                continue;
            }
            let def = Def::of(form);
            let names = def.and_then(|d| d.names().ok()).unwrap_or_default();
            if def.is_some_and(|d| d.value_is_function()) {
                live.retain(|n| !names.contains(n));
            }
            if self.reads(form, live) {
                return true;
            }
            live.retain(|n| !names.contains(n));
        }
        false
    }

    fn form(&self, items: &[Syntax], names: &[&str]) -> bool {
        let head = items.first().and_then(Syntax::as_symbol);
        match head {
            Some("quote") => false,
            Some("def" | "var") if items.len() == 3 => self.reads(&items[2], names),
            Some("defn" | "defmacro") if items.len() >= 3 => {
                self.function(&items[2], &items[3..], names)
            }
            Some("fn") if items.len() >= 2 => self.function(&items[1], &items[2..], names),
            Some("let" | "let*" | "letrec") if items.len() >= 2 => {
                let Some(pairs) = items[1].as_list_or_tuple() else {
                    return self.any(items, names);
                };
                let mut live = names.to_vec();
                if head == Some("letrec") {
                    for pair in pairs.chunks(2) {
                        live = hidden(&live, &pair[0]);
                    }
                }
                self.values(pairs, &mut live) || self.body(&items[2..], &live)
            }
            Some("each") => match each_parts(items) {
                Some((var, coll, body)) => {
                    self.reads(&items[coll], names) || self.function(&items[var], body, names)
                }
                None => self.any(items, names),
            },
            Some("block") => self.body(&items[1..], names),
            // A sequence outside a body: up to epoch 16 a def there binds its
            // name again for the forms after it in the sequence.
            Some("begin" | "do" | "forever") => self.body(&items[1..], names),
            Some("when" | "unless" | "while" | "repeat") if items.len() >= 2 => {
                self.reads(&items[1], names) || self.body(&items[2..], names)
            }
            Some("parameterize") if items.len() >= 2 => {
                self.reads(&items[1], names) || self.body(&items[2..], names)
            }
            _ => match self.binding_macro(items, names) {
                Some(read) => read,
                None => self.any(items, names),
            },
        }
    }

    /// Whether the value of a binding pair in `pairs` reads one of `live`,
    /// each value read past the names its own and later pairs bind. Every
    /// pair's names leave `live`.
    fn values(&self, pairs: &[Syntax], live: &mut Vec<&str>) -> bool {
        for pair in pairs.chunks(2) {
            if pair.len() == 2 && self.reads(&pair[1], live) {
                return true;
            }
            *live = hidden(live, &pair[0]);
        }
        false
    }
}

/// `names` without each name `pattern` binds.
fn hidden<'n>(names: &[&'n str], pattern: &Syntax) -> Vec<&'n str> {
    without(names, &bound(pattern).unwrap_or_default())
}

/// Whether the symbol `symbol` reads `name`: the name itself, or a qualified
/// symbol whose first segment it is, as `x:key` reads `x`.
pub(crate) fn names_read(symbol: &str, name: &str) -> bool {
    symbol
        .strip_prefix(name)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(':'))
}

/// `names` without each of `hidden`.
pub(crate) fn without<'t>(names: &[&'t str], hidden: &[&str]) -> Vec<&'t str> {
    names
        .iter()
        .copied()
        .filter(|n| !hidden.contains(n))
        .collect()
}

/// The names a binding pattern binds, each without its `@`, or `None` when a
/// template fills one in.
pub(crate) fn bound(pattern: &Syntax) -> Option<Vec<&str>> {
    let mut names = Vec::new();
    collect(pattern, &mut names).then_some(names)
}

/// Push the names `pattern` binds onto `names`. False at an unquote.
fn collect<'t>(pattern: &'t Syntax, names: &mut Vec<&'t str>) -> bool {
    match &pattern.kind {
        SyntaxKind::Symbol(s) => {
            let name = s.as_str();
            let name = name.strip_prefix('@').unwrap_or(name);
            if !matches!(name, "_" | "&" | "&opt" | "&keys" | "&named" | ".") {
                names.push(name);
            }
            true
        }
        SyntaxKind::Unquote(_) | SyntaxKind::UnquoteSplicing(_) => false,
        SyntaxKind::Quote(_) => true,
        kind => kind.children().iter().all(|c| collect(c, names)),
    }
}

/// The names the defs standing directly in `forms` bind, through every
/// `begin`. A def whose names a template fills in adds none.
pub(super) fn body_defs(forms: &[Syntax]) -> Vec<&str> {
    let mut names = Vec::new();
    for form in forms {
        if is_begin(form) {
            names.extend(body_defs(&form.kind.children()[1..]));
        } else if let Some(names_of) = Def::of(form).and_then(|d| d.names().ok()) {
            names.extend(names_of);
        }
    }
    names
}

/// Where an `each` form keeps its parts: the index of the variable and of the
/// collection, and the body. `(each x xs body…)` and `(each x in xs body…)`
/// both, read as the prelude macro reads them.
pub(crate) fn each_parts(items: &[Syntax]) -> Option<(usize, usize, &[Syntax])> {
    if items.len() >= 5 && items[2].is_symbol("in") {
        Some((1, 3, &items[4..]))
    } else if items.len() >= 3 {
        Some((1, 2, &items[3..]))
    } else {
        None
    }
}
