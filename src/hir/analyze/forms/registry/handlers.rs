// audited: 2026-10-06
//! The special-form handlers: thunks that adapt `Analyzer` methods to the shape the registry dispatches through.
//!
//! That shape is [`FormHandler`](super::FormHandler).
//!
//! docs/impl/hir.md

use super::super::super::Analyzer;
use crate::hir::expr::Hir;
use crate::syntax::{Span, Syntax, SyntaxKind};

// Two shapes: whole-form (`items`) and body-only (`&items[1..]`).

pub(super) fn sf_if(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_if(items, span)
}
pub(super) fn sf_let(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_let(items, span)
}
pub(super) fn sf_letrec(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_letrec(items, span)
}
pub(super) fn sf_fn(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_lambda(items, span)
}
pub(super) fn sf_begin(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_begin(&items[1..], span)
}
pub(super) fn sf_file_body(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_file_body(&items[1..], span)
}
pub(super) fn sf_block(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_block(&items[1..], span)
}
pub(super) fn sf_break(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_break(&items[1..], span)
}
pub(super) fn sf_var(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_define(items, span)
}
pub(super) fn sf_def(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_const(items, span)
}
pub(super) fn sf_assign(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_assign(items, span)
}
pub(super) fn sf_while(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_while(items, span)
}
pub(super) fn sf_and(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_and(&items[1..], span)
}
pub(super) fn sf_or(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_or(&items[1..], span)
}
pub(super) fn sf_match(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_match(items, span)
}
pub(super) fn sf_cond(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_cond(items, span)
}
pub(super) fn sf_eval(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_eval(items, span)
}
pub(super) fn sf_environment(
    a: &mut Analyzer,
    items: &[Syntax],
    span: Span,
) -> Result<Hir, String> {
    a.analyze_environment(items, span)
}
pub(super) fn sf_parameterize(
    a: &mut Analyzer,
    items: &[Syntax],
    span: Span,
) -> Result<Hir, String> {
    a.analyze_parameterize(items, span)
}
pub(super) fn sf_silence(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_silence(items, span)
}
pub(super) fn sf_muffle(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_muffle(items, span)
}
pub(super) fn sf_attune_assert(
    a: &mut Analyzer,
    items: &[Syntax],
    span: Span,
) -> Result<Hir, String> {
    a.analyze_attune_assert(items, span)
}
pub(super) fn sf_silence_assert(
    a: &mut Analyzer,
    items: &[Syntax],
    span: Span,
) -> Result<Hir, String> {
    a.analyze_silence_assert(items, span)
}
pub(super) fn sf_numeric_assert(
    a: &mut Analyzer,
    items: &[Syntax],
    span: Span,
) -> Result<Hir, String> {
    a.analyze_numeric_assert(items, span)
}
pub(super) fn sf_immutable_assert(
    a: &mut Analyzer,
    items: &[Syntax],
    span: Span,
) -> Result<Hir, String> {
    a.analyze_immutable_assert(items, span)
}
pub(super) fn sf_unicode(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    a.analyze_unicode(items, span)
}
pub(super) fn sf_quote(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    if items.len() != 2 {
        return Err(format!("{}: quote requires 1 argument", span));
    }
    a.analyze_quoted_datum(&items[1], span)
}
pub(super) fn sf_signal(a: &mut Analyzer, items: &[Syntax], span: Span) -> Result<Hir, String> {
    if items.len() != 2 {
        return Err(format!("{}: signal requires exactly 1 argument", span));
    }
    let keyword = match &items[1].kind {
        SyntaxKind::Keyword(k) => *k,
        _ => {
            return Err(format!(
                "{}: signal requires a keyword argument, got {}",
                items[1].span,
                items[1].kind_label()
            ));
        }
    };
    a.declare_signal(&keyword, &items[1].span)?;
    Ok(Hir::silent(
        crate::hir::expr::HirKind::Keyword(keyword.to_string()),
        span,
    ))
}
pub(super) fn sf_splice(_a: &mut Analyzer, _items: &[Syntax], span: Span) -> Result<Hir, String> {
    Err(format!(
        "{}: `;` is the splice operator, not a comment character. Use `#` for comments.",
        span
    ))
}
pub(super) fn sf_import_file(
    a: &mut Analyzer,
    items: &[Syntax],
    span: Span,
) -> Result<Hir, String> {
    a.analyze_import_file(items, span)
}
pub(super) fn sf_meta_location(
    a: &mut Analyzer,
    items: &[Syntax],
    span: Span,
) -> Result<Hir, String> {
    a.analyze_meta_location(items, span)
}
