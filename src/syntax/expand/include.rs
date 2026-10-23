// audited: 2026-10-22
//! Splice an include's file into the sequence that holds it, and the entry points that expand into a sequence.
//!
//! docs/include.md
//! src/syntax/expand/AGENTS.md
//!
//! `Includes` (src/pipeline/compile/include.rs) resolves an include, checks the
//! chain of files being included, and reads the file. This file stamps the
//! forms read and expands each where the include stood.

use super::*;
use crate::pipeline::{IncludeName, IncludeRead};

impl Expander {
    /// Expand every macro in `syntax`. A position that holds one form takes
    /// the forms of an include inside a `begin`, which bears the span and the
    /// scopes of `syntax`.
    pub fn expand(
        &mut self,
        syntax: Syntax,
        symbols: &mut SymbolTable,
        vm: &mut VM,
    ) -> Result<Syntax, String> {
        let mut out = Vec::with_capacity(1);
        self.expand_into(syntax, &mut out, symbols, vm)?;
        if let [one] = out[..] {
            return Ok(one);
        }
        let begin = Syntax::with_scope_slice(
            SyntaxKind::Symbol(self.arena.text("begin")),
            syntax.span,
            syntax.scope_slice(),
        );
        out.insert(0, begin);
        Ok(Syntax::with_scope_slice(
            SyntaxKind::List(self.arena.nodes(&out)),
            syntax.span,
            syntax.scope_slice(),
        ))
    }

    /// Expand a file's top-level `forms` in order, each include among them
    /// spliced where it stands.
    pub(crate) fn expand_forms(
        &mut self,
        forms: Vec<Syntax>,
        symbols: &mut SymbolTable,
        vm: &mut VM,
    ) -> Result<Vec<Syntax>, String> {
        let mut out = Vec::with_capacity(forms.len());
        for form in forms {
            self.expand_into(form, &mut out, symbols, vm)?;
        }
        Ok(out)
    }

    /// Each include the unit read, in the order it read them, which the
    /// store's key reads.
    pub(crate) fn take_include_reads(&mut self) -> Vec<IncludeRead> {
        self.includes.take_read()
    }

    /// Splice the file `name` names in place of the include `form`. Each form
    /// read takes every scope of the include's head, so it resolves names as
    /// code written at the include does, and expands onto `out`.
    pub(super) fn splice_include(
        &mut self,
        form: Syntax,
        name: IncludeName,
        out: &mut Vec<Syntax>,
        symbols: &mut SymbolTable,
        vm: &mut VM,
    ) -> Result<(), String> {
        let (read, mark) = self
            .includes
            .enter(&self.arena, &form, name, vm, &self.eval_meta)?;
        let scopes = form.as_list().map_or(&[][..], |items| items[0].scopes());
        let mut spliced = Ok(());
        for item in read {
            let item = scopes
                .iter()
                .fold(item, |item, scope| self.add_scope_recursive(item, *scope));
            spliced = self.expand_into(item, out, symbols, vm);
            if spliced.is_err() {
                break;
            }
        }
        self.includes.leave(mark);
        spliced
    }
}
