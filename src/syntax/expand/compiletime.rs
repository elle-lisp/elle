//! begin-for-syntax: compile-time definition evaluation.
//!
//! Evaluates `(def <symbol> <expr>)` forms at expansion time and stores
//! the results in `Expander.compile_time_env`. The forms produce no
//! runtime code — `begin-for-syntax` expands to nil.
//!
//! Only `(def <symbol> <expr>)` is supported. Destructuring defs, `var`
//! forms, and bare expressions are rejected with a clear error.

use super::Expander;
use crate::symbol::SymbolTable;
use crate::syntax::{Span, Syntax, SyntaxKind};
use crate::vm::VM;

impl Expander {
    /// Handle `(begin-for-syntax form ...)`.
    ///
    /// Each form must be `(def <symbol> <expr>)`. The value expression is
    /// compiled and executed via `eval_syntax`, and the result is stored in
    /// `self.compile_time_env` under the symbol name. Returns `nil`.
    pub(super) fn handle_begin_for_syntax(
        &mut self,
        items: &[Syntax],
        span: &Span,
        symbols: &mut SymbolTable,
        vm: &mut VM,
    ) -> Result<Syntax, String> {
        // items[0] is the `begin-for-syntax` symbol; items[1..] are the forms.
        for form in &items[1..] {
            self.process_bfs_form(form, span, symbols, vm)?;
        }
        Ok(Syntax::new(SyntaxKind::Nil, span.clone()))
    }

    fn process_bfs_form(
        &mut self,
        form: &Syntax,
        outer_span: &Span,
        symbols: &mut SymbolTable,
        vm: &mut VM,
    ) -> Result<(), String> {
        // Form must be a list starting with `def`.
        let parts = match form.as_list() {
            Some(p) => p,
            None => {
                return Err(format!(
                    "{}: begin-for-syntax: only (def <symbol> <expr>) is supported",
                    form.span
                ))
            }
        };

        // Must be exactly (def <symbol> <expr>) — 3 elements.
        if parts.len() != 3 || parts[0].as_symbol() != Some("def") {
            return Err(format!(
                "{}: begin-for-syntax: only (def <symbol> <expr>) is supported",
                form.span
            ));
        }

        // Name at position 1 must be a plain symbol (no destructuring).
        let name = match parts[1].as_symbol() {
            Some(n) => n.to_string(),
            None => {
                return Err(format!(
                    "{}: begin-for-syntax: binding name must be a plain symbol, got {}",
                    parts[1].span,
                    parts[1].kind_label()
                ))
            }
        };

        // Value expression at position 2. Do NOT pre-expand — eval_syntax
        // calls expander.expand() internally. Pre-expanding would double-expand.
        let value_syntax = parts[2].clone();

        // Evaluate the value expression in the macro VM.
        let value = crate::pipeline::eval_syntax(value_syntax, self, symbols, vm).map_err(|e| {
            format!(
                "{}: begin-for-syntax: error evaluating def {}: {}",
                outer_span, name, e
            )
        })?;

        // Store in compile-time environment.
        self.compile_time_env.insert(name, value);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::register_primitives;
    use crate::reader::read_syntax;
    use crate::symbol::SymbolTable;
    use crate::syntax::Expander;
    use crate::vm::VM;

    fn setup() -> (SymbolTable, VM) {
        let mut symbols = SymbolTable::new();
        let mut vm = VM::new();
        register_primitives(&mut vm, &mut symbols);
        (symbols, vm)
    }

    #[test]
    fn bfs_non_def_rejected() {
        // (begin-for-syntax (+ 1 2)) must fail
        let mut expander = Expander::new();
        let (mut symbols, mut vm) = setup();
        expander.load_prelude(&mut symbols, &mut vm).unwrap();

        let src = "(begin-for-syntax (+ 1 2))";
        let syn = read_syntax(src, "<test>").unwrap();
        let result = expander.expand(syn, &mut symbols, &mut vm);
        assert!(result.is_err(), "non-def form should be rejected");
        let msg = result.unwrap_err();
        assert!(
            msg.contains("begin-for-syntax"),
            "error should mention begin-for-syntax: {}",
            msg
        );
    }

    #[test]
    fn bfs_destructuring_def_rejected() {
        // (begin-for-syntax (def (a b) 42)) must fail
        let mut expander = Expander::new();
        let (mut symbols, mut vm) = setup();
        expander.load_prelude(&mut symbols, &mut vm).unwrap();

        let src = "(begin-for-syntax (def (a b) 42))";
        let syn = read_syntax(src, "<test>").unwrap();
        let result = expander.expand(syn, &mut symbols, &mut vm);
        assert!(result.is_err());
    }

    #[test]
    fn bfs_stores_value_in_env() {
        // (begin-for-syntax (def my-val 42)) should store "my-val" in compile_time_env
        let mut expander = Expander::new();
        let (mut symbols, mut vm) = setup();
        expander.load_prelude(&mut symbols, &mut vm).unwrap();

        let src = "(begin-for-syntax (def my-val 42))";
        let syn = read_syntax(src, "<test>").unwrap();
        expander.expand(syn, &mut symbols, &mut vm).unwrap();

        assert!(
            expander.compile_time_env.contains_key("my-val"),
            "compile_time_env should contain my-val"
        );
        let val = expander.compile_time_env["my-val"];
        assert_eq!(val, crate::value::Value::int(42));
    }

    #[test]
    fn bfs_returns_nil() {
        // (begin-for-syntax (def x 1)) should expand to nil syntax
        use crate::syntax::SyntaxKind;
        let mut expander = Expander::new();
        let (mut symbols, mut vm) = setup();
        expander.load_prelude(&mut symbols, &mut vm).unwrap();

        let src = "(begin-for-syntax (def x 1))";
        let syn = read_syntax(src, "<test>").unwrap();
        let result = expander.expand(syn, &mut symbols, &mut vm).unwrap();
        assert!(
            matches!(result.kind, SyntaxKind::Nil),
            "begin-for-syntax should expand to nil"
        );
    }

    #[test]
    fn bfs_clone_resets_env() {
        // Cloning an Expander that has compile_time_env entries should
        // produce an Expander with an empty compile_time_env.
        let mut expander = Expander::new();
        let (mut symbols, mut vm) = setup();
        expander.load_prelude(&mut symbols, &mut vm).unwrap();

        let src = "(begin-for-syntax (def helper 99))";
        let syn = read_syntax(src, "<test>").unwrap();
        expander.expand(syn, &mut symbols, &mut vm).unwrap();
        assert!(!expander.compile_time_env.is_empty());

        let cloned = expander.clone();
        assert!(
            cloned.compile_time_env.is_empty(),
            "cloned Expander should have empty compile_time_env"
        );
    }
}
