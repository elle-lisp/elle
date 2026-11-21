// audited: 2026-11-20
//! The epoch 14 rule applied to a syntax tree: a call's duration in milliseconds becomes a `:timeout` in seconds.
//!
//! docs/epochs.md
//!
//! `crate::epoch::seconds` says what each shape of duration becomes. This file
//! finds the duration in a call and writes the answer into the tree.

use super::instantiate_template;
use crate::epoch::rules::TimeArg;
use crate::epoch::seconds::{Millis, Seconds, SecondsLiteral, Written};
use crate::syntax::{Span, Syntax, SyntaxArena, SyntaxKind};

/// Rewrite the millisecond duration the call `syntax` gave at `place` as a
/// `:timeout` in seconds. Returns the number of nodes rewritten: 0 when the
/// call does not have the shape the rule names.
pub(super) fn to_seconds(
    arena: &SyntaxArena,
    syntax: &mut Syntax,
    place: TimeArg,
) -> Result<usize, String> {
    let SyntaxKind::List(items) = &syntax.kind else {
        return Ok(0);
    };
    let span = syntax.span;
    let mut items = items.to_vec();
    match place {
        TimeArg::Keyword => {
            let Some(at) = (1..items.len().saturating_sub(1))
                .find(|&i| matches!(&items[i].kind, SyntaxKind::Keyword(k) if *k == "timeout"))
            else {
                return Ok(0);
            };
            let value = items[at + 1];
            let Seconds::Write(written) = Seconds::of(millis_of(&value), place) else {
                return Ok(0);
            };
            items[at + 1] = written_node(arena, written, value, span)?;
        }
        TimeArg::Last { arity, .. } => {
            if items.len() != arity + 1 {
                return Ok(0);
            }
            let value = items.pop().expect("a call of arity 1 or more");
            match Seconds::of(millis_of(&value), place) {
                Seconds::Keep => return Ok(0),
                Seconds::Drop => {}
                Seconds::Write(written) => {
                    items.push(Syntax::keyword(arena, "timeout", value.span));
                    items.push(written_node(arena, written, value, span)?);
                }
            }
        }
    }
    syntax.kind = SyntaxKind::List(arena.nodes(&items));
    Ok(1)
}

/// The shape of a duration argument, as the conversion tells shapes apart.
fn millis_of(value: &Syntax) -> Millis {
    match value.kind {
        SyntaxKind::Int(n) => Millis::Int(n),
        SyntaxKind::Float(x) => Millis::Float(x),
        SyntaxKind::Nil => Millis::Nil,
        _ => Millis::Expr,
    }
}

/// The node a duration written in seconds becomes: a literal where `value`
/// stood, or `value` wrapped to divide at run time. The wrap carries `span`,
/// the call's, as a `Replace` template does.
fn written_node(
    arena: &SyntaxArena,
    written: Written,
    value: Syntax,
    span: Span,
) -> Result<Syntax, String> {
    match written {
        Written::Literal(SecondsLiteral::Int(n)) => Ok(Syntax::new(SyntaxKind::Int(n), value.span)),
        Written::Literal(SecondsLiteral::Float(x)) => {
            Ok(Syntax::new(SyntaxKind::Float(x), value.span))
        }
        Written::Wrap(template) => instantiate_template(arena, template, &[value], &span),
    }
}
