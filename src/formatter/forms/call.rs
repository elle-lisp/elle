// audited: 2026-10-27
//! The generic function-call fallback. Every other special-form rule defers
//! here when its arity guard fails or the head isn't a recognised form.
//!
//! docs/fmt.md

use super::parens;
use crate::formatter::config::FormatterConfig;
use crate::formatter::doc::Doc;
use crate::formatter::format::{format_annotated, format_pair};
use crate::formatter::trivia::AnnotatedSyntax;
use crate::syntax::SyntaxKind;

/// Generic function call: inline if it fits, and otherwise the arguments hang
/// from the head and fill each line before they wrap.
///
/// ```lisp
/// (f a b c)          # fits on one line
/// (f a               # doesn't fit — first arg stays with head
///    b               #   remaining args align to first arg
///    c)
/// ```
///
/// Keyword-value pairs (`:key val`) are kept together as units so they break
/// as one. Past half the line width the arguments indent +2 from the paren
/// instead ([`Doc::hang`]).
pub(in crate::formatter) fn format_generic_call(
    children: &[AnnotatedSyntax],
    source: &str,
    config: &FormatterConfig,
) -> Doc {
    let Some((head, args)) = children.split_first() else {
        return Doc::text("()");
    };
    let head = format_annotated(head, source, config);

    let mut units = build_arg_units(args, source, config).into_iter();
    let Some(first) = units.next() else {
        return parens(head);
    };
    let rest = Doc::concat(units.map(|unit| Doc::concat([Doc::Break, unit]).group()));

    parens(Doc::concat([head, Doc::hang(first, rest)]))
}

/// Build argument units for generic calls, grouping `:keyword value` pairs.
///
/// A keyword followed by a non-keyword argument forms a single doc unit
/// joined by a space. Consecutive keywords or trailing keywords stand alone.
fn build_arg_units(args: &[AnnotatedSyntax], source: &str, config: &FormatterConfig) -> Vec<Doc> {
    let is_keyword = |node: &AnnotatedSyntax| matches!(node.syntax.kind, SyntaxKind::Keyword(_));
    let mut units = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args.get(i + 1) {
            Some(value) if is_keyword(&args[i]) && !is_keyword(value) => {
                units.push(format_pair(&args[i], value, source, config));
                i += 2;
            }
            _ => {
                units.push(format_annotated(&args[i], source, config));
                i += 1;
            }
        }
    }
    units
}
