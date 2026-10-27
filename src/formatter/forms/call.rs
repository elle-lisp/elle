// audited: 2026-10-25
//! The generic function-call fallback. Every other special-form rule defers
//! here when its arity guard fails or the head isn't a recognised form.
//!
//! docs/fmt.md

use crate::formatter::config::FormatterConfig;
use crate::formatter::doc::Doc;
use crate::formatter::format::{format_annotated, format_pair};
use crate::formatter::trivia::AnnotatedSyntax;
use crate::syntax::SyntaxKind;

/// Generic function call: try inline; break with args aligned to first arg.
///
/// Head and first arg stay on the same line. When breaking, subsequent
/// args align to the first arg's column. Keyword-value pairs (`:key val`)
/// are kept together as units so they break as one.
///
/// ```lisp
/// (f a b c)          # fits on one line
/// (f a               # doesn't fit — first arg stays with head
///    b               #   remaining args align to first arg
///    c)
/// ```
///
/// The head sits inside a Nest, so a comment above it, or the arguments after
/// a comment beside it, indent +2 from the open paren.
pub(in crate::formatter) fn format_generic_call(
    children: &[AnnotatedSyntax],
    source: &str,
    config: &FormatterConfig,
) -> Doc {
    let Some((head, args)) = children.split_first() else {
        return Doc::text("()");
    };
    let head = format_annotated(head, source, config).nest(1);

    let args = match args {
        [] => return Doc::concat([Doc::text("("), head, Doc::text(")")]),
        // Head + one arg: Align so arg indents to first-arg column
        [arg] => Doc::concat([format_annotated(arg, source, config)]).group(),
        // Columnar fill: args align to the first arg's column and
        // fill-wrap greedily (each element independently wraps).
        _ => Doc::fill(build_arg_units(args, source, config)),
    };

    Doc::concat([
        Doc::text("("),
        head,
        Doc::Space,
        Doc::align(args),
        Doc::text(")"),
    ])
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
