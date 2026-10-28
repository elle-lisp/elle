// audited: 2026-10-27
//! `if`, and the prelude macros that keep a header beside the head and break a
//! body below it, by one table.
//!
//! docs/fmt.md

use super::define::format_bindings;
use super::{format_body, format_generic_call, is_trivial, parens};
use crate::formatter::config::FormatterConfig;
use crate::formatter::doc::Doc;
use crate::formatter::format::{format_annotated, format_trailing_trivia};
use crate::formatter::trivia::AnnotatedSyntax;
use crate::syntax::SyntaxKind;

/// The arguments that stay beside the head.
#[derive(Clone, Copy)]
enum Header {
    /// This many arguments, each laid out as itself.
    Args(usize),
    /// One vector of name and value pairs, laid out as `let` lays out its own.
    Bindings,
}

impl Header {
    fn len(self) -> usize {
        match self {
            Header::Args(n) => n,
            Header::Bindings => 1,
        }
    }
}

/// Which bodies may join the header on one line.
#[derive(Clone, Copy)]
enum Body {
    /// A single body form that is trivial.
    Forms,
    /// Any number of branches, every one of them trivial.
    Branches,
}

/// Each body macro, with its header and its body.
const MACROS: &[(&str, Header, Body)] = &[
    ("when", Header::Args(1), Body::Forms),
    ("unless", Header::Args(1), Body::Forms),
    ("when-let", Header::Bindings, Body::Forms),
    ("when-ok", Header::Bindings, Body::Forms),
    ("if-let", Header::Bindings, Body::Branches),
    ("with-temp-dir", Header::Args(1), Body::Forms),
    ("with-allocator", Header::Args(1), Body::Forms),
    ("ffi/with-stack", Header::Args(1), Body::Forms),
    ("repeat", Header::Args(1), Body::Forms),
    ("gate!", Header::Args(2), Body::Forms),
    ("with", Header::Args(3), Body::Forms),
];

/// `(if test then else?)`: the test is the header, and the branches the body.
pub(in crate::formatter) fn format_if(
    children: &[AnnotatedSyntax],
    source: &str,
    config: &FormatterConfig,
) -> Doc {
    format_header_body(children, Header::Args(1), Body::Branches, source, config)
}

/// The layout of the body macro `name`, or None when the table has no row for
/// it.
pub(in crate::formatter) fn format_body_macro(
    name: &str,
    children: &[AnnotatedSyntax],
    source: &str,
    config: &FormatterConfig,
) -> Option<Doc> {
    let &(_, header, body) = MACROS.iter().find(|(row, ..)| *row == name)?;
    Some(format_header_body(children, header, body, source, config))
}

/// The head and its header on one line, then the body. The body joins that
/// line when `body` allows it and the whole form fits, and otherwise breaks,
/// +2. A form with no body takes the call layout.
fn format_header_body(
    children: &[AnnotatedSyntax],
    header: Header,
    body: Body,
    source: &str,
    config: &FormatterConfig,
) -> Doc {
    if children.len() < header.len() + 2 {
        return format_generic_call(children, source, config);
    }
    let (heads, bodies) = children.split_at(header.len() + 1);

    let mut line = vec![format_annotated(&heads[0], source, config)];
    for arg in &heads[1..] {
        line.push(Doc::Space);
        line.push(match header {
            Header::Bindings if matches!(arg.syntax.kind, SyntaxKind::Array(_)) => Doc::concat([
                format_bindings(arg, source, config),
                format_trailing_trivia(arg),
            ]),
            _ => format_annotated(arg, source, config),
        });
    }
    let line = Doc::concat(line);

    let joins = match body {
        Body::Forms => bodies.len() == 1 && is_trivial(&bodies[0]),
        Body::Branches => bodies.iter().all(is_trivial),
    };
    parens(if joins {
        let bodies = bodies.iter().map(|b| format_annotated(b, source, config));
        Doc::concat([line, Doc::Break, Doc::intersperse(bodies)]).group()
    } else {
        Doc::concat([line, Doc::HardBreak, format_body(bodies, source, config)])
    })
}
