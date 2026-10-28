// audited: 2026-10-27
//! Every line inside a form starts right of the form's open delimiter, and
//! the check that `assert_formats` runs on every output it is given.
//!
//! docs/fmt.md
//!
//! The counter-factual: a wrapped call whose arguments start at its own `(`.
//! The output is idempotent and fits the line, so only a check on the
//! column of each line start sees that the arguments read as a new form.

use crate::formatter::comments::lex_for_format;
use crate::reader::{lexicon_for, SyntaxReader};
use crate::syntax::{Syntax, SyntaxHeap, SyntaxKind};

/// Each line of `text` that starts at or left of the open delimiter of a form
/// it sits inside. The text of a multi-line string is exempt.
pub(super) fn lines_left_of_open(text: &str) -> Vec<String> {
    let lexed = lex_for_format(text, "<margin>", lexicon_for(text).unwrap()).unwrap();
    let mut home = SyntaxHeap::new();
    let forms = if lexed.tokens.is_empty() {
        Vec::new()
    } else {
        SyntaxReader::with_byte_offsets(
            lexed.tokens,
            lexed.locations,
            lexed.lengths,
            lexed.byte_offsets,
            home.arena(),
        )
        .read_all()
        .unwrap()
    };
    let mut opens = Vec::new();
    let mut strings = Vec::new();
    for form in &forms {
        collect_spans(form, &mut opens, &mut strings);
    }
    let column = |at: usize| at - text[..at].rfind('\n').map_or(0, |n| n + 1);
    let inside = |spans: &[(usize, usize)], at: usize| -> Vec<(usize, usize)> {
        spans
            .iter()
            .copied()
            .filter(|&(start, end)| start < at && at < end)
            .collect()
    };

    let mut bad = Vec::new();
    let mut line_start = 0;
    for line in text.split_inclusive('\n') {
        let indent = line.len() - line.trim_start().len();
        let at = line_start + indent;
        line_start += line.len();
        if line.trim().is_empty() || !inside(&strings, at).is_empty() {
            continue;
        }
        if inside(&opens, at)
            .iter()
            .any(|&(start, _)| column(start) >= indent)
        {
            bad.push(line.trim_end().to_string());
        }
    }
    bad
}

/// The byte range of every form with an open delimiter under `node`, and of
/// every string literal.
fn collect_spans(
    node: &Syntax,
    opens: &mut Vec<(usize, usize)>,
    strings: &mut Vec<(usize, usize)>,
) {
    let range = (node.span.start as usize, node.span.end as usize);
    match &node.kind {
        SyntaxKind::List(cs)
        | SyntaxKind::Array(cs)
        | SyntaxKind::ArrayMut(cs)
        | SyntaxKind::Struct(cs)
        | SyntaxKind::StructMut(cs)
        | SyntaxKind::Set(cs)
        | SyntaxKind::SetMut(cs)
        | SyntaxKind::Bytes(cs)
        | SyntaxKind::BytesMut(cs) => {
            opens.push(range);
            for child in cs.iter() {
                collect_spans(child, opens, strings);
            }
        }
        SyntaxKind::Quote(inner)
        | SyntaxKind::Quasiquote(inner)
        | SyntaxKind::Unquote(inner)
        | SyntaxKind::UnquoteSplicing(inner)
        | SyntaxKind::Splice(inner) => collect_spans(inner, opens, strings),
        SyntaxKind::String(_) | SyntaxKind::StringMut(_) => strings.push(range),
        _ => {}
    }
}

#[test]
fn the_check_finds_an_argument_at_its_calls_paren() {
    assert_eq!(lines_left_of_open("(f a\nb)"), ["b)"]);
    assert_eq!(lines_left_of_open("(def x\n  (g a\n  b))"), ["  b))"]);
}

#[test]
fn the_check_finds_a_comment_at_its_forms_paren() {
    assert_eq!(lines_left_of_open("(f a\n# c\n b)"), ["# c"]);
}

#[test]
fn the_check_passes_a_line_right_of_every_open_paren() {
    assert!(lines_left_of_open("(f a\n   b)\n(g\n  [1\n   2])").is_empty());
}

#[test]
fn the_check_skips_the_text_of_a_multi_line_string() {
    assert!(lines_left_of_open("(f \"a\nb\")").is_empty());
}
