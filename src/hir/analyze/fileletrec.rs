// audited: 2026-11-19
//! A file's top-level forms as one letrec: classifying each form, and the bindings the file adds of its own.
//!
//! docs/bindings.md
//! docs/signals/inference.md
//!
//! `analyze_file_letrec`, in the `letrec` submodule, analyzes the forms in
//! order. Each `def` binds its name from its own form onward, as in a function
//! body, and a later `def` of a name makes a new binding.

use super::*;
use crate::syntax::{Syntax, SyntaxKind};

/// Classify a macro-expanded top-level form for `analyze_file_letrec`:
/// `(def name value)` / `(var name value)` are bindings, `(signal :kw)` is a
/// signal declaration, and everything else is a bare expression (gensym-named).
/// Used by the file front-end and by the `%file-body` thunk-body form so both
/// see identical file-scope semantics.
pub fn classify_form(syntax: &Syntax) -> FileForm<'_> {
    if let SyntaxKind::List(items) = &syntax.kind {
        if items.len() == 2 {
            if let Some(head) = items[0].as_symbol() {
                if head == "signal" {
                    return FileForm::Signal(&items[1]);
                }
            }
        }
        if items.len() == 3 {
            if let Some(head) = items[0].as_symbol() {
                match head {
                    "def" => return FileForm::Def(syntax, &items[1], &items[2]),
                    "var" => return FileForm::Var(syntax, &items[1], &items[2]),
                    _ => {}
                }
            }
        }
    }
    FileForm::Expr(syntax)
}

mod letrec;
mod nested;

impl<'a> Analyzer<'a> {
    /// Analyze an internal `(%file-body form…)` node: the body of a whole-module
    /// test thunk. Classifies its forms and runs the SAME `analyze_file_letrec`
    /// a real file gets, so its bindings live as long as a file's. Inserted by
    /// `pipeline::compile::transforms::whole_module_transform`; never written
    /// by users.
    pub(crate) fn analyze_file_body(
        &mut self,
        items: &[Syntax],
        span: Span,
    ) -> Result<Hir, String> {
        let forms: Vec<FileForm> = items.iter().map(classify_form).collect();
        self.analyze_file_letrec(forms, span)
    }

    /// A synthetic file binding named `prefix` and the next count, for a
    /// letrec entry that binds no name of its own.
    fn file_gensym(&mut self, prefix: &str, counter: &mut u32) -> Binding {
        let name = format!("{}{}", prefix, counter);
        *counter += 1;
        let binding = self.bind(&name, &[], BindingScope::Local);
        self.arena.get_mut(binding).is_synthetic = true;
        binding
    }
}
