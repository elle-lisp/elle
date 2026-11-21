// audited: 2026-11-20
//! The epoch 17 rule's reading of a tree: which defs outside a body assign, which move into a let, and which stay.
//!
//! docs/epochs/bodies.md
//! docs/bindings.md
//!
//! The rule reads a file twice. The first walk answers the defs that assign,
//! [`Assigns`]. The second walk reads the tree those assigns leave and
//! answers a [`Plan`]. A placement names a run of defs by the path to the form
//! that holds it, and a note names a def the rule leaves. The tree migration
//! applies both through [`tree`], and `elle rewrite` turns the same answers
//! into edits of the text, so the compiler and the tool cannot read a def's
//! place differently.
//!
//! [`walk`] says how each form the rule knows places its forms, and [`reads`]
//! answers whether a form reads a name. The epoch 18 rule's reading
//! (`forward`) asks [`reads`] too.

use super::rules::{migrations_in_range, MigrationRule};
use crate::syntax::{Span, Syntax};

pub(crate) mod reads;
pub(crate) mod tree;
mod walk;

/// A run of defs that moves into one let.
pub(crate) struct Placement {
    /// The child index at each step from a top-level form down to the form
    /// whose forms hold the run.
    pub path: Vec<usize>,
    /// The index of the run's first def among those forms.
    pub first: usize,
    /// How many defs the run holds.
    pub count: usize,
    /// The index past the last form the let takes in. The forms after the
    /// run, up to this one, are the let's body.
    pub end: usize,
    /// Whether the let takes the place of the form itself: a `begin` outside a
    /// body, whose run starts at its first form.
    pub replaces: bool,
}

/// A def the rule leaves as written, and why.
pub(crate) struct Note {
    pub span: Span,
    pub reason: String,
}

/// Every placement and every note the rule makes over one file's forms.
pub(crate) struct Plan {
    pub placements: Vec<Placement>,
    pub notes: Vec<Note>,
}

/// The defs that assign a binding made before their home: the form that holds
/// the def, directly in its body.
pub(crate) struct Assigns {
    /// The path to each def that becomes an `assign`.
    pub defs: Vec<Vec<usize>>,
    /// Each binding the defs assign, in the order of the walk.
    pub decls: Vec<Decl>,
}

/// A mutable binding made just before a home, which the defs there assign.
pub(crate) struct Decl {
    /// The path to the home.
    pub home: Vec<usize>,
    pub name: String,
    /// Whether the binding holds the binding of the name before it, rather
    /// than `nil`.
    pub seeded: bool,
    /// The span of the first def that assigns it.
    pub span: Span,
}

impl Decl {
    /// The `def` that makes the binding, as the text writes it.
    pub(crate) fn text(&self) -> String {
        let value = if self.seeded { &self.name } else { "nil" };
        format!("(def @{} {value})", self.name)
    }
}

/// Whether migrating from epoch `from` to epoch `to` crosses the epoch that
/// adds the rule.
pub fn applies(from: u64, to: u64) -> bool {
    migrations_in_range(from, to)
        .flat_map(|m| m.rules.iter())
        .any(|rule| matches!(rule, MigrationRule::DefIntoLet))
}

/// Read `forms`, a file's top-level forms, and answer the defs that assign.
pub(crate) fn assigns(forms: &[Syntax]) -> Assigns {
    walk::Walker::assigns(forms)
}

/// Read `forms`, a file's top-level forms after the assigns, and plan the
/// rule over them.
pub(crate) fn plan(forms: &[Syntax]) -> Plan {
    walk::Walker::over(forms)
}

/// What a def is: its form and the shape the rule reads.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Def,
    Var,
    Defn,
}

/// A `def`, `var` or `defn` form, as the rule reads it.
#[derive(Clone, Copy)]
pub(crate) struct Def<'t> {
    pub node: &'t Syntax,
    pub kind: Kind,
    /// The form's elements: the head, the name, then the value or the
    /// parameters and body of a `defn`.
    pub items: &'t [Syntax],
}

impl<'t> Def<'t> {
    /// `node` as a def, or `None` when it is no def of a shape the rule reads.
    pub(crate) fn of(node: &'t Syntax) -> Option<Def<'t>> {
        let items = node.as_list()?;
        let kind = match items.first()?.as_symbol()? {
            "def" if items.len() == 3 => Kind::Def,
            "var" if items.len() == 3 => Kind::Var,
            "defn" if items.len() >= 3 => Kind::Defn,
            _ => return None,
        };
        Some(Def { node, kind, items })
    }

    /// The name or pattern the def binds.
    pub(crate) fn name(&self) -> &'t Syntax {
        &self.items[1]
    }

    /// Whether the def binds a pattern rather than one name.
    pub(crate) fn binds_pattern(&self) -> bool {
        self.name().as_symbol().is_none()
    }

    /// The names the def binds, or why the rule cannot read them.
    pub(crate) fn names(&self) -> Result<Vec<&'t str>, &'static str> {
        if self.kind == Kind::Var && self.binds_pattern() {
            return Err("it is a var that binds a pattern");
        }
        reads::bound(self.name()).ok_or("a template fills in its name")
    }

    /// Whether the def's value reads one of `names`. A `defn`'s value is the
    /// function its parameters and body make.
    pub(crate) fn value_reads(&self, names: &[&str]) -> bool {
        match self.kind {
            Kind::Defn => reads::Scan::PLAIN.function(&self.items[2], &self.items[3..], names),
            Kind::Def | Kind::Var => reads::Scan::PLAIN.reads(&self.items[2], names),
        }
    }

    /// Whether the def's value is a function: a `defn`, or a `fn` form.
    pub(crate) fn value_is_function(&self) -> bool {
        self.kind == Kind::Defn
            || self.items[2]
                .as_list()
                .and_then(<[Syntax]>::first)
                .is_some_and(|head| head.is_symbol("fn"))
    }

    /// The name the let reads back when no form follows the run.
    pub(crate) fn plain_name(&self) -> &'t str {
        let name = self.name().as_symbol().unwrap_or_default();
        name.strip_prefix('@').unwrap_or(name)
    }

    /// Whether the let must write `@` before the name: a `var` whose name
    /// does not already carry it.
    pub(crate) fn needs_at(&self) -> bool {
        self.kind == Kind::Var && !self.name().as_symbol().is_some_and(|n| n.starts_with('@'))
    }
}

/// Whether `node` is a `begin` or `do`, which opens no scope.
pub(crate) fn is_begin(node: &Syntax) -> bool {
    matches!(
        node.as_list()
            .and_then(<[Syntax]>::first)
            .and_then(Syntax::as_symbol),
        Some("begin" | "do")
    )
}

/// The node `path` names: the top-level form at its first index, then a
/// child at each index after.
pub(crate) fn node_at<'t>(forms: &'t [Syntax], path: &[usize]) -> &'t Syntax {
    let (first, rest) = path
        .split_first()
        .expect("a placement's path is never empty");
    rest.iter()
        .fold(&forms[*first], |node, &i| &node.kind.children()[i])
}
