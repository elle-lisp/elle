//! Macro call expansion via VM evaluation
//!
//! The macro body is compiled and executed in the real VM. Arguments are
//! quoted and bound via `let`. The result Value is converted back to Syntax
//! via `from_value()`.
//!
//! Known limitations:
//! - `from_value()` creates Syntax with empty scope sets, so scope marks
//!   from the original arguments are lost through the Value round-trip.
//!   PR 3 (sets-of-scopes hygiene) must address this.
//! - Macros cannot return improper lists (e.g. `(cons 1 2)`). The
//!   `from_value()` conversion requires proper lists.
//! - `gensym` currently returns a string, not a symbol. Using gensym
//!   results in quasiquote templates produces string literals, not
//!   symbol bindings. See #306.

use super::{Expander, MacroDef, SyntaxKind, MAX_MACRO_EXPANSION_DEPTH};
use crate::symbol::SymbolTable;
use crate::syntax::Syntax;
use crate::vm::VM;

impl Expander {
    pub(super) fn expand_macro_call(
        &mut self,
        macro_def: &MacroDef,
        args: &[Syntax],
        call_site: &Syntax,
        symbols: &mut SymbolTable,
        vm: &mut VM,
    ) -> Result<Syntax, String> {
        // Check arity
        if args.len() != macro_def.params.len() {
            return Err(format!(
                "Macro '{}' expects {} arguments, got {}",
                macro_def.name,
                macro_def.params.len(),
                args.len()
            ));
        }

        // Recursion guard
        self.expansion_depth += 1;
        if self.expansion_depth > MAX_MACRO_EXPANSION_DEPTH {
            self.expansion_depth -= 1;
            return Err(format!(
                "macro expansion depth exceeded {} (possible infinite expansion)",
                MAX_MACRO_EXPANSION_DEPTH
            ));
        }

        let result = self.expand_macro_call_inner(macro_def, args, call_site, symbols, vm);
        self.expansion_depth -= 1;
        result
    }

    fn expand_macro_call_inner(
        &mut self,
        macro_def: &MacroDef,
        args: &[Syntax],
        call_site: &Syntax,
        symbols: &mut SymbolTable,
        vm: &mut VM,
    ) -> Result<Syntax, String> {
        let span = call_site.span.clone();

        // Build let-expression: (let ((p1 'a1) (p2 'a2)) body)
        let bindings: Vec<Syntax> = macro_def
            .params
            .iter()
            .zip(args)
            .map(|(param, arg)| {
                let quoted_arg =
                    Syntax::new(SyntaxKind::Quote(Box::new(arg.clone())), span.clone());
                Syntax::new(
                    SyntaxKind::List(vec![
                        Syntax::new(SyntaxKind::Symbol(param.clone()), span.clone()),
                        quoted_arg,
                    ]),
                    span.clone(),
                )
            })
            .collect();

        let let_expr = Syntax::new(
            SyntaxKind::List(vec![
                Syntax::new(SyntaxKind::Symbol("let".to_string()), span.clone()),
                Syntax::new(SyntaxKind::List(bindings), span.clone()),
                macro_def.template.clone(),
            ]),
            span.clone(),
        );

        // Compile and execute in the VM
        let result_value = crate::pipeline::eval_syntax(let_expr, self, symbols, vm)?;

        // Convert result back to Syntax
        let result_syntax = Syntax::from_value(&result_value, symbols, span)?;

        // Add intro scope for hygiene
        let intro_scope = self.fresh_scope();
        let hygienized = self.add_scope_recursive(result_syntax, intro_scope);

        // Continue expanding the result
        self.expand(hygienized, symbols, vm)
    }
}
