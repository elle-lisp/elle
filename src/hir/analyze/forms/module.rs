// audited: 2026-11-09
//! `meta/location`: the form that answers the file, line and column it was written at.
//!
//! docs/modules.md

use super::*;

impl<'a> Analyzer<'a> {
    /// `(meta/location)`: `{:file :line :col}` for the form itself.
    ///
    /// The list's span is the location, and a list a macro builds carries the
    /// span of the macro call (src/syntax/convert.rs), so the `meta/location`
    /// in an expansion names the caller. The head symbol keeps the template's
    /// own span and would name the macro's file instead.
    pub(crate) fn analyze_meta_location(
        &mut self,
        items: &[Syntax],
        span: Span,
    ) -> Result<Hir, String> {
        if items.len() != 1 {
            return Err(format!("{}: meta/location takes no arguments", span));
        }
        let file = match span.source_path() {
            Some(path) => HirKind::String(path),
            None => HirKind::Nil,
        };
        let field = |key: &str, value: HirKind| {
            [
                CallArg {
                    expr: Hir::silent(HirKind::Keyword(key.to_string()), span),
                    spliced: false,
                },
                CallArg {
                    expr: Hir::silent(value, span),
                    spliced: false,
                },
            ]
        };
        let args = [
            field("file", file),
            field("line", HirKind::Int(i64::from(span.line))),
            field("col", HirKind::Int(i64::from(span.col))),
        ]
        .concat();
        let func = Hir::silent(HirKind::Var(self.resolve_primitive("struct")), span);
        Ok(Hir::silent(
            HirKind::Call {
                func: Box::new(func),
                args,
                is_tail: false,
            },
            span,
        ))
    }
}
