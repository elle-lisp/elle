// audited: 2026-11-20
//! The epoch 16 rule's reading of a module: whether its forms fold into its last fn form, and how.
//!
//! docs/epochs.md
//! docs/modules.md
//!
//! [`plan`] reads a file's top-level forms and answers a [`Fold`]. A loader's
//! tree migration applies it through [`apply`], and `elle rewrite` turns the
//! same plan into edits of the text, so the two cannot fold a module apart.

use super::placement::reads::bound;
use super::rules::{migrations_in_range, MigrationRule};
use crate::syntax::{Syntax, SyntaxArena, SyntaxKind};

/// How the forms before the function join it.
pub(crate) enum Shape {
    /// The forms move into the function's body, after its docstring.
    Body,
    /// A form spells a parameter's name, so the forms open a function that
    /// takes every argument as `name` and applies the old function to them.
    Apply { name: String },
}

/// A module's forms and the function they fold into.
pub(crate) struct Fold<'t> {
    /// The forms before the function.
    pub lead: &'t [Syntax],
    /// The function: the last top-level form.
    pub function: &'t Syntax,
    /// The function's elements: `fn`, the parameters, then the body.
    pub items: &'t [Syntax],
    /// Whether the body opens with a docstring, which stays first.
    pub doc: bool,
    pub shape: Shape,
}

/// Whether migrating from epoch `from` to epoch `to` crosses the epoch that
/// adds the rule.
pub fn applies(from: u64, to: u64) -> bool {
    migrations_in_range(from, to)
        .flat_map(|m| m.rules.iter())
        .any(|rule| matches!(rule, MigrationRule::FoldModule))
}

/// Read `forms`, a file's top-level forms, and plan the fold. `None` when the
/// file holds one form, or its last form is no `fn` form with a body.
pub(crate) fn plan(forms: &[Syntax]) -> Option<Fold<'_>> {
    let (function, lead) = forms.split_last()?;
    let items = function.as_list()?;
    if lead.is_empty() || items.len() < 3 || !items[0].is_symbol("fn") {
        return None;
    }
    if !matches!(items[1].kind, SyntaxKind::List(_) | SyntaxKind::Array(_)) {
        return None;
    }
    let doc = items.len() > 3 && matches!(items[2].kind, SyntaxKind::String(_));
    // A parameter a template fills in has no name to check, so it wraps.
    let shadowed = match bound(&items[1]) {
        Some(params) => lead.iter().any(|f| spells(f, &|s| params.contains(&s))),
        None => true,
    };
    let shape = if shadowed {
        let taken = |name: &str| forms.iter().any(|f| spells(f, &|s| s == name));
        let name = std::iter::once("args".to_string())
            .chain((1..).map(|n| format!("args-{n}")))
            .find(|name| !taken(name))
            .expect("the names never run out");
        Shape::Apply { name }
    } else {
        Shape::Body
    };
    Some(Fold {
        lead,
        function,
        items,
        doc,
        shape,
    })
}

/// Fold `forms`, a module's top-level forms, into one form when [`plan`]
/// answers a fold. Answers the number of rewrites: one or none.
pub(crate) fn apply(arena: &SyntaxArena, forms: &mut Vec<Syntax>) -> usize {
    let Some(folded) = plan(forms).map(|fold| fold.tree(arena)) else {
        return 0;
    };
    *forms = vec![folded];
    1
}

impl Fold<'_> {
    /// The one form the fold makes, spanned at the function, so an error in
    /// its head points where the author wrote it.
    fn tree(&self, arena: &SyntaxArena) -> Syntax {
        let span = self.function.span;
        let symbol = |name: &str| Syntax::symbol(arena, name, span);
        let mut items = vec![self.items[0]];
        match &self.shape {
            Shape::Body => {
                let head = if self.doc { 3 } else { 2 };
                items.extend_from_slice(&self.items[1..head]);
                items.extend_from_slice(self.lead);
                items.extend_from_slice(&self.items[head..]);
            }
            Shape::Apply { name } => {
                items.push(Syntax::array(arena, &[symbol("&"), symbol(name)], span));
                items.extend_from_slice(self.lead);
                let call = [symbol("apply"), *self.function, symbol(name)];
                items.push(Syntax::list(arena, &call, span));
            }
        }
        Syntax::list(arena, &items, span)
    }
}

/// Whether `node`, quoted parts included, holds a symbol that names one
/// `name` answers for: the symbol without its `@`, or the first segment of a
/// qualified symbol, as `x:key` names `x`.
fn spells(node: &Syntax, name: &dyn Fn(&str) -> bool) -> bool {
    match &node.kind {
        SyntaxKind::Symbol(s) => {
            let s = s.as_str();
            let s = s.strip_prefix('@').unwrap_or(s);
            name(s) || s.split_once(':').is_some_and(|(first, _)| name(first))
        }
        kind => kind.children().iter().any(|c| spells(c, name)),
    }
}
