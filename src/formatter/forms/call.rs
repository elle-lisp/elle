//! The generic function-call fallback. Every other special-form rule defers
//! here when its arity guard fails or the head isn't a recognised form.

use crate::formatter::config::FormatterConfig;
use crate::formatter::doc::Doc;
use crate::formatter::format::format_annotated;
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
pub(in crate::formatter) fn format_generic_call(
    children: &[AnnotatedSyntax],
    source: &str,
    config: &FormatterConfig,
) -> Doc {
    if children.is_empty() {
        return Doc::text("()");
    }

    if children.len() == 1 {
        // Head only
        return Doc::concat([
            Doc::text("("),
            format_annotated(&children[0], source, config),
            Doc::text(")"),
        ]);
    }

    if children.len() == 2 {
        // Head + one arg: Align so arg indents to first-arg column
        let head = format_annotated(&children[0], source, config);
        let arg = format_annotated(&children[1], source, config);
        return Doc::concat([
            Doc::text("("),
            head,
            Doc::text(" "),
            Doc::align(Doc::concat([arg]).group()),
            Doc::text(")"),
        ]);
    }

    let head = format_annotated(&children[0], source, config);

    // Build arg units: keyword-value pairs are joined with a space,
    // positional args stand alone.
    let arg_units = build_arg_units(&children[1..], source, config);

    // Columnar fill: args align to the first arg's column and
    // fill-wrap greedily (each element independently wraps).
    Doc::concat([
        Doc::text("("),
        head,
        Doc::text(" "),
        Doc::align(Doc::fill(arg_units)),
        Doc::text(")"),
    ])
}

/// Build argument units for generic calls, grouping `:keyword value` pairs.
///
/// A keyword followed by a non-keyword argument forms a single doc unit
/// joined by a space. Consecutive keywords or trailing keywords stand alone.
fn build_arg_units(args: &[AnnotatedSyntax], source: &str, config: &FormatterConfig) -> Vec<Doc> {
    let mut units = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let doc = format_annotated(&args[i], source, config);
        if matches!(args[i].syntax.kind, SyntaxKind::Keyword(_)) {
            // Keyword — pair with next arg if it's not also a keyword
            if i + 1 < args.len() && !matches!(args[i + 1].syntax.kind, SyntaxKind::Keyword(_)) {
                let val = format_annotated(&args[i + 1], source, config);
                units.push(Doc::concat([doc, Doc::text(" "), val]));
                i += 2;
                continue;
            }
        }
        units.push(doc);
        i += 1;
    }
    units
}
