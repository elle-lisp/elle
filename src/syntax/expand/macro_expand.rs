//! Macro call expansion via VM evaluation
//!
//! The macro body is compiled and executed in the real VM. Arguments are
//! wrapped as `Value::syntax(arg)` via `SyntaxLiteral` and bound via `let`.
//! The result Value is converted back to Syntax via `from_value()`.
//!
//! Scope preservation: arguments are wrapped as syntax objects so their
//! scope sets survive the Value round-trip. `from_value()` unwraps syntax
//! objects back to Syntax, preserving scopes. `add_scope_recursive` then
//! stamps the intro scope on the result, including unwrapped argument nodes.
//!
//! Known limitations:
//! - Macros cannot return improper lists (e.g. `(cons 1 2)`). The
//!   `from_value()` conversion requires proper lists.

use super::{Expander, MacroDef, SyntaxKind, MAX_MACRO_EXPANSION_DEPTH};
use crate::symbol::SymbolTable;
use crate::syntax::{Span, Syntax};
use crate::value::Value;
use crate::vm::VM;

/// Wrap a macro argument for binding in the let-expression.
/// Atoms are quoted to preserve semantics. Symbols and compounds
/// are wrapped as syntax objects to preserve scope sets.
fn wrap_macro_arg(arg: &Syntax, span: &Span) -> Syntax {
    match &arg.kind {
        SyntaxKind::Nil
        | SyntaxKind::Bool(_)
        | SyntaxKind::Int(_)
        | SyntaxKind::Float(_)
        | SyntaxKind::String(_)
        | SyntaxKind::Keyword(_) => {
            Syntax::new(SyntaxKind::Quote(Box::new(arg.clone())), span.clone())
        }
        _ => Syntax::new(
            SyntaxKind::SyntaxLiteral(Value::syntax(arg.clone())),
            span.clone(),
        ),
    }
}

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
        if macro_def.rest_param.is_some() {
            if args.len() < macro_def.params.len() {
                return Err(format!(
                    "Macro '{}' expects at least {} arguments, got {}",
                    macro_def.name,
                    macro_def.params.len(),
                    args.len()
                ));
            }
        } else if args.len() != macro_def.params.len() {
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

        // Build let-expression: (let ((p1 arg1) (p2 arg2)) body)
        // Symbols and compound forms are wrapped as Value::syntax(arg) via
        // SyntaxLiteral to preserve scope sets through the Value round-trip.
        // Atoms (nil, bool, int, float, string, keyword) are quoted normally —
        // they don't participate in binding resolution and wrapping them as
        // syntax objects would change their runtime semantics (e.g., #f wrapped
        // in a syntax object becomes truthy).
        // Bind fixed params
        let mut bindings: Vec<Syntax> = macro_def
            .params
            .iter()
            .zip(&args[..macro_def.params.len()])
            .map(|(param, arg)| {
                let arg_expr = wrap_macro_arg(arg, &span);
                Syntax::new(
                    SyntaxKind::List(vec![
                        Syntax::new(SyntaxKind::Symbol(param.clone()), span.clone()),
                        arg_expr,
                    ]),
                    span.clone(),
                )
            })
            .collect();

        // Bind rest param if present
        if let Some(ref rest_name) = macro_def.rest_param {
            let rest_args = &args[macro_def.params.len()..];
            // Build (list arg1 arg2 ...) expression
            let mut list_elems = vec![Syntax::new(
                SyntaxKind::Symbol("list".to_string()),
                span.clone(),
            )];
            for arg in rest_args {
                list_elems.push(wrap_macro_arg(arg, &span));
            }
            let list_expr = Syntax::new(SyntaxKind::List(list_elems), span.clone());
            bindings.push(Syntax::new(
                SyntaxKind::List(vec![
                    Syntax::new(SyntaxKind::Symbol(rest_name.clone()), span.clone()),
                    list_expr,
                ]),
                span.clone(),
            ));
        }

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
