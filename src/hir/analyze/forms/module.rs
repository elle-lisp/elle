// audited: 2026-10-06
//! The forms that need the file a form was written in: `import-file`, which
//! loads a path relative to it, and `meta/location`, which answers it.
//!
//! docs/modules.md

use super::*;
use crate::syntax::{ScopeId, SyntaxHeap};

/// Whether `path` names a shared library, which loads as a plugin.
fn is_library(path: &str) -> bool {
    matches!(
        crate::path::extension(path),
        Some("so") | Some("dylib") | Some("dll")
    )
}

impl<'a> Analyzer<'a> {
    /// `(import-file path)`: the load of the file `path` names.
    ///
    /// The handler builds the load as a form and analyzes it. A literal path
    /// joins the writer's directory now and names its loader, so the form is
    /// `(import/load-file "/abs/path")`. A computed path is joined and its
    /// loader picked when the form runs:
    ///
    /// ```text
    /// (let [p (path/join DIR ARG)  e (path/extension p)]
    ///   (if (or (= e "so") (= e "dylib") (= e "dll"))
    ///     (import/load-plugin p)
    ///     (import/load-file p)))
    /// ```
    ///
    /// Code with no file drops the join, and the loader resolves a relative
    /// path against the working directory. `path/join` keeps an absolute
    /// argument as it stands.
    ///
    /// Every symbol the handler writes carries [`ScopeId::COMPILER`], so it
    /// skips a user's local binding of the same name, as a macro template's
    /// symbol does. `ARG` is the user's own syntax and resolves where it was
    /// written.
    pub(crate) fn analyze_import_file(
        &mut self,
        items: &[Syntax],
        span: Span,
    ) -> Result<Hir, String> {
        if items.len() != 2 {
            return Err(format!(
                "{}: import-file: expected 1 argument, got {}",
                span,
                items.len() - 1
            ));
        }
        let dir = span
            .source_path()
            .and_then(|f| crate::path::parent(&f).map(str::to_string));
        // The analyzer holds no syntax arena, so the built form lives on a heap
        // of its own for the length of its analysis. The HIR copies every name
        // and string out of the syntax, so nothing it returns points here.
        let (_home, arena) = SyntaxHeap::with_arena();
        let sym = |name: &str| Syntax::symbol_scoped(&arena, name, span, &[ScopeId::COMPILER]);
        let string = |s: &str| Syntax::string(&arena, s, span);
        let list = |items: &[Syntax]| Syntax::list(&arena, items, span);

        let form = if let SyntaxKind::String(path) = &items[1].kind {
            let path = match &dir {
                Some(dir) => crate::path::normalize(&crate::path::join(&[dir, path])),
                None => crate::path::absolute(path)
                    .map_err(|e| format!("{}: import-file: {}", span, e))?,
            };
            let loader = if is_library(&path) {
                "import/load-plugin"
            } else {
                "import/load-file"
            };
            list(&[sym(loader), string(&path)])
        } else {
            let target = match &dir {
                Some(dir) => list(&[sym("path/join"), string(dir), items[1]]),
                None => items[1],
            };
            let ext_is = |suffix: &str| list(&[sym("="), sym("e"), string(suffix)]);
            list(&[
                sym("let"),
                Syntax::array(
                    &arena,
                    &[
                        sym("p"),
                        target,
                        sym("e"),
                        list(&[sym("path/extension"), sym("p")]),
                    ],
                    span,
                ),
                list(&[
                    sym("if"),
                    list(&[sym("or"), ext_is("so"), ext_is("dylib"), ext_is("dll")]),
                    list(&[sym("import/load-plugin"), sym("p")]),
                    list(&[sym("import/load-file"), sym("p")]),
                ]),
            ])
        };
        self.analyze_expr(&form)
    }

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
