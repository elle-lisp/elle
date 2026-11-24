// audited: 2026-11-24
//! W006: a macro expansion that holds one compound argument at two positions outside a quote.
//!
//! docs/macros.md
//! docs/cookbook/lint-rules.md

use crate::lint::diagnostics::{Diagnostic, ARGUMENT_PLACED_TWICE};
use crate::reader::SourceLoc;
use crate::syntax::{Syntax, SyntaxKind};
use crate::value::{SymbolId, Value};

/// Whether a copy of `arg` costs a compile anything: a list, an array, a
/// struct or a set form. A name or a literal, the empty list included, does
/// not, so a template may place one anywhere.
fn compound(arg: &Syntax) -> bool {
    match &arg.kind {
        SyntaxKind::List(items) => !items.is_empty(),
        SyntaxKind::Array(_)
        | SyntaxKind::ArrayMut(_)
        | SyntaxKind::Struct(_)
        | SyntaxKind::StructMut(_)
        | SyntaxKind::Set(_)
        | SyntaxKind::SetMut(_) => true,
        _ => false,
    }
}

/// Whether `value` is a `(quote …)` list, whose contents do not run. The head
/// is a plain symbol, or the syntax object quasiquote builds for a template
/// symbol.
fn quote_form(value: Value) -> bool {
    let Some(pair) = value.as_pair() else {
        return false;
    };
    let head = pair.first;
    head.as_symbol() == Some(SymbolId::of("quote"))
        || head
            .as_syntax()
            .is_some_and(|s| s.as_symbol() == Some("quote"))
}

/// Warn at each compound argument of `macro_name` that `result`, the value
/// its transformer answered, holds more than once outside a quote.
///
/// The expander hands each argument to the transformer as one value, and a
/// template that places it puts that same value in the result. So identity
/// counts the copies, where a span would not: a form one macro builds for
/// another carries the outer call's span, as every other form it builds does.
/// A syntax object the transformer built itself is not walked.
pub(crate) fn check_argument_placed_twice(
    macro_name: &str,
    args: &[Syntax],
    values: &[Value],
    result: Value,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let watched: Vec<(&Syntax, Value)> = args
        .iter()
        .zip(values.iter().copied())
        .filter(|(arg, _)| compound(arg))
        .collect();
    if watched.is_empty() {
        return;
    }
    let mut counts = vec![0usize; watched.len()];
    let mut stack = vec![result];
    while let Some(value) = stack.pop() {
        if let Some(i) = watched.iter().position(|(_, v)| v.bit_identical(value)) {
            counts[i] += 1;
            continue;
        }
        if value.as_syntax().is_some() || quote_form(value) {
            continue;
        }
        if value.as_pair().is_some() {
            stack.extend(value.list_to_vec().unwrap_or_default());
        } else if let Some(items) = value.as_array().or(value.as_set()) {
            stack.extend_from_slice(items);
        } else if let Some(items) = value.as_struct() {
            stack.extend(items.iter().map(|(_, v)| *v));
        } else if let Some(items) = value.array_mut_read() {
            stack.extend(items.iter().copied());
        } else if let Some(items) = value.set_mut_read() {
            stack.extend(items.iter().copied());
        } else if let Some(items) = value.struct_mut_read() {
            stack.extend(items.iter().map(|(_, v)| *v));
        }
    }
    for ((arg, _), count) in watched.iter().zip(counts) {
        if count < 2 {
            continue;
        }
        let location = SourceLoc::from_line_col(arg.span.line as usize, arg.span.col as usize);
        let mut diag = Diagnostic::warn(
            ARGUMENT_PLACED_TWICE,
            format!(
                "macro '{macro_name}' places this argument at {count} positions outside a quote"
            ),
            Some(location),
        );
        diag.suggestions.push(
            "bind the argument once with `let`, or join the arms that compute one value \
             with `block` and `break`"
                .to_string(),
        );
        diagnostics.push(diag);
    }
}
