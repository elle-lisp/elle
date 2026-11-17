// audited: 2026-11-17
//! Where a `def` may stand: directly in a body, or in a `begin` that stands directly in one.
//!
//! docs/bindings.md
//!
//! A body marks each of its forms before it analyzes it, and `analyze_expr`
//! takes the mark, so the mark reaches the form itself and none of its
//! operands. A `begin` passes its own mark on to its forms.

use super::*;

/// Where the form the analyzer is dispatching stands.
#[derive(Default, Clone, Copy)]
pub(super) struct Standing {
    /// Whether the next form `analyze_expr` meets stands directly in a body.
    /// A body sets it before each of its forms, and `analyze_expr` takes it.
    next_in_body: bool,
    /// Whether the form being dispatched stands directly in a body.
    in_body: bool,
    /// The form the one being dispatched stands in, past any `begin`: what a
    /// misplaced `def` names.
    standing_in: Option<Syntax>,
    /// The nearest form around the one being analyzed that is no `begin`.
    enclosing: Option<Syntax>,
}

impl<'a> Analyzer<'a> {
    /// Analyze `item`, a form that stands directly in a body.
    pub(super) fn analyze_in_body(&mut self, item: &Syntax) -> Result<Hir, String> {
        self.standing.next_in_body = true;
        self.analyze_expr(item)
    }

    /// Take the mark for `syntax`, the form `analyze_expr` is about to
    /// dispatch. Answers what [`Analyzer::leave_form`] restores.
    pub(super) fn enter_form(&mut self, syntax: &Syntax) -> Standing {
        let outer = self.standing;
        self.standing = Standing {
            next_in_body: false,
            in_body: outer.next_in_body,
            standing_in: outer.enclosing,
            enclosing: if is_begin(syntax) {
                outer.enclosing
            } else {
                Some(*syntax)
            },
        };
        Standing {
            next_in_body: false,
            ..outer
        }
    }

    /// Restore what [`Analyzer::enter_form`] answered, once the form is done.
    pub(super) fn leave_form(&mut self, outer: Standing) {
        self.standing = outer;
    }

    /// Whether the form being dispatched stands directly in a body.
    pub(super) fn in_body(&self) -> bool {
        self.standing.in_body
    }

    /// Run `analyze` over the value of the `def` form `form`, so a `def` in
    /// the value names `form` as the form it stands in.
    pub(super) fn in_def_value<T>(
        &mut self,
        form: Option<Syntax>,
        analyze: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let outer = std::mem::replace(&mut self.standing.enclosing, form);
        let result = analyze(self);
        self.standing.enclosing = outer;
        result
    }

    /// Refuse the `def` or `var` form being dispatched unless it stands
    /// directly in a body. The message names the form it stands in.
    pub(super) fn check_placement(&self, form: &str, span: Span) -> Result<(), String> {
        if self.standing.in_body {
            return Ok(());
        }
        let place = match &self.standing.standing_in {
            Some(outer) => match outer.as_list().and_then(<[Syntax]>::first) {
                Some(head) => match head.as_symbol() {
                    Some(name) => format!("the ({} ...) form at {}", name, outer.span),
                    None => format!("the call at {}", outer.span),
                },
                None => format!("the {} at {}", outer.kind_label(), outer.span),
            },
            None => "no body".to_string(),
        };
        Err(format!(
            "{}: a {} binds only directly in a body, and this one stands in {}",
            span, form, place
        ))
    }
}

/// Whether `syntax` is a `begin`, which passes a misplaced `def`'s blame to
/// the form around it.
fn is_begin(syntax: &Syntax) -> bool {
    matches!(
        syntax
            .as_list()
            .and_then(<[Syntax]>::first)
            .and_then(Syntax::as_symbol),
        Some("begin" | "do")
    )
}
