//! AST optimization passes
//!
//! Peephole optimizations that transform the AST before bytecode generation.

use super::ast::Expr;
use crate::symbol::SymbolTable;
use crate::value::Value;

/// Apply all optimization passes to an expression
pub fn optimize(expr: &mut Expr, symbols: &SymbolTable) {
    optimize_length_zero_check(expr, symbols);
}

/// Peephole optimization: (= (length x) 0) -> (empty? x)
/// Also handles: (= 0 (length x)) -> (empty? x)
///
/// This transforms O(n) length checks into O(1) empty checks.
fn optimize_length_zero_check(expr: &mut Expr, symbols: &SymbolTable) {
    // First, recursively optimize children
    match expr {
        Expr::If { cond, then, else_ } => {
            optimize_length_zero_check(cond, symbols);
            optimize_length_zero_check(then, symbols);
            optimize_length_zero_check(else_, symbols);
        }
        Expr::Begin(exprs)
        | Expr::Block(exprs)
        | Expr::And(exprs)
        | Expr::Or(exprs)
        | Expr::Xor(exprs) => {
            for e in exprs.iter_mut() {
                optimize_length_zero_check(e, symbols);
            }
        }
        Expr::Call { func, args, .. } => {
            optimize_length_zero_check(func, symbols);
            for arg in args.iter_mut() {
                optimize_length_zero_check(arg, symbols);
            }

            // Check for the pattern: (= (length x) 0) or (= 0 (length x))
            let should_optimize = if let Expr::GlobalVar(func_sym) = func.as_ref() {
                if let Some("=") = symbols.name(*func_sym) {
                    args.len() == 2
                } else {
                    false
                }
            } else {
                false
            };

            if should_optimize {
                // Try both orderings: (= (length x) 0) and (= 0 (length x))
                let optimized = try_optimize_length_zero(&args[0], &args[1], symbols)
                    .or_else(|| try_optimize_length_zero(&args[1], &args[0], symbols));

                if let Some(new_expr) = optimized {
                    *expr = new_expr;
                }
            }
        }
        Expr::Lambda { body, .. } => {
            optimize_length_zero_check(body, symbols);
        }
        Expr::Let { bindings, body } | Expr::Letrec { bindings, body } => {
            for (_, init) in bindings.iter_mut() {
                optimize_length_zero_check(init, symbols);
            }
            optimize_length_zero_check(body, symbols);
        }
        Expr::Set { value, .. } | Expr::Define { value, .. } => {
            optimize_length_zero_check(value, symbols);
        }
        Expr::While { cond, body } => {
            optimize_length_zero_check(cond, symbols);
            optimize_length_zero_check(body, symbols);
        }
        Expr::For { iter, body, .. } => {
            optimize_length_zero_check(iter, symbols);
            optimize_length_zero_check(body, symbols);
        }
        Expr::Match {
            value,
            patterns,
            default,
        } => {
            optimize_length_zero_check(value, symbols);
            for (_, body) in patterns.iter_mut() {
                optimize_length_zero_check(body, symbols);
            }
            if let Some(d) = default {
                optimize_length_zero_check(d, symbols);
            }
        }
        Expr::Try {
            body,
            catch,
            finally,
        } => {
            optimize_length_zero_check(body, symbols);
            if let Some((_, handler)) = catch {
                optimize_length_zero_check(handler, symbols);
            }
            if let Some(f) = finally {
                optimize_length_zero_check(f, symbols);
            }
        }
        Expr::Cond { clauses, else_body } => {
            for (cond, body) in clauses.iter_mut() {
                optimize_length_zero_check(cond, symbols);
                optimize_length_zero_check(body, symbols);
            }
            if let Some(e) = else_body {
                optimize_length_zero_check(e, symbols);
            }
        }
        Expr::HandlerCase { body, handlers } => {
            optimize_length_zero_check(body, symbols);
            for (_, _, handler) in handlers.iter_mut() {
                optimize_length_zero_check(handler, symbols);
            }
        }
        Expr::HandlerBind { handlers, body } => {
            for (_, handler) in handlers.iter_mut() {
                optimize_length_zero_check(handler, symbols);
            }
            optimize_length_zero_check(body, symbols);
        }
        Expr::Throw { value } => {
            optimize_length_zero_check(value, symbols);
        }
        Expr::Quote(_) | Expr::Quasiquote(_) | Expr::Unquote(_) => {
            // Don't optimize inside quoted expressions
        }
        Expr::DefMacro { body, .. } => {
            optimize_length_zero_check(body, symbols);
        }
        Expr::Module { body, .. } => {
            optimize_length_zero_check(body, symbols);
        }
        // Leaf nodes - nothing to do
        Expr::Literal(_)
        | Expr::Var(..)
        | Expr::GlobalVar(_)
        | Expr::Import { .. }
        | Expr::ModuleRef { .. } => {}

        Expr::Yield(expr) => {
            optimize_length_zero_check(expr, symbols);
        }
    }
}

/// Try to match (length x) as first arg and 0 as second arg
/// Returns Some(optimized_expr) if pattern matches, None otherwise
fn try_optimize_length_zero(
    maybe_length: &Expr,
    maybe_zero: &Expr,
    symbols: &SymbolTable,
) -> Option<Expr> {
    // Check if second arg is 0
    if !matches!(maybe_zero, Expr::Literal(Value::Int(0))) {
        return None;
    }

    // Check if first arg is (length x)
    if let Expr::Call { func, args, .. } = maybe_length {
        if let Expr::GlobalVar(func_sym) = func.as_ref() {
            if let Some("length") = symbols.name(*func_sym) {
                if args.len() == 1 {
                    // Found the pattern! Transform to (empty? x)
                    let empty_sym = symbols.get("empty?").unwrap_or_else(|| {
                        // This shouldn't happen in practice since empty? is a builtin
                        panic!("empty? symbol not found in symbol table")
                    });

                    return Some(Expr::Call {
                        func: Box::new(Expr::GlobalVar(empty_sym)),
                        args: vec![args[0].clone()],
                        tail: false,
                    });
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_length_zero_optimization() {
        let mut symbols = SymbolTable::new();
        let eq_sym = symbols.intern("=");
        let length_sym = symbols.intern("length");
        let empty_sym = symbols.intern("empty?");
        let x_sym = symbols.intern("x");

        // Create (= (length x) 0)
        let mut expr = Expr::Call {
            func: Box::new(Expr::GlobalVar(eq_sym)),
            args: vec![
                Expr::Call {
                    func: Box::new(Expr::GlobalVar(length_sym)),
                    args: vec![Expr::GlobalVar(x_sym)],
                    tail: false,
                },
                Expr::Literal(Value::Int(0)),
            ],
            tail: false,
        };

        optimize(&mut expr, &symbols);

        // Should be transformed to (empty? x)
        match expr {
            Expr::Call { func, args, .. } => {
                assert!(matches!(func.as_ref(), Expr::GlobalVar(sym) if *sym == empty_sym));
                assert_eq!(args.len(), 1);
                assert!(matches!(&args[0], Expr::GlobalVar(sym) if *sym == x_sym));
            }
            _ => panic!("Expected Call expression"),
        }
    }

    #[test]
    fn test_length_zero_optimization_reversed() {
        let mut symbols = SymbolTable::new();
        let eq_sym = symbols.intern("=");
        let length_sym = symbols.intern("length");
        let empty_sym = symbols.intern("empty?");
        let x_sym = symbols.intern("x");

        // Create (= 0 (length x))
        let mut expr = Expr::Call {
            func: Box::new(Expr::GlobalVar(eq_sym)),
            args: vec![
                Expr::Literal(Value::Int(0)),
                Expr::Call {
                    func: Box::new(Expr::GlobalVar(length_sym)),
                    args: vec![Expr::GlobalVar(x_sym)],
                    tail: false,
                },
            ],
            tail: false,
        };

        optimize(&mut expr, &symbols);

        // Should be transformed to (empty? x)
        match expr {
            Expr::Call { func, args, .. } => {
                assert!(matches!(func.as_ref(), Expr::GlobalVar(sym) if *sym == empty_sym));
                assert_eq!(args.len(), 1);
                assert!(matches!(&args[0], Expr::GlobalVar(sym) if *sym == x_sym));
            }
            _ => panic!("Expected Call expression"),
        }
    }

    #[test]
    fn test_non_zero_comparison_not_optimized() {
        let mut symbols = SymbolTable::new();
        let eq_sym = symbols.intern("=");
        let length_sym = symbols.intern("length");
        let x_sym = symbols.intern("x");

        // Create (= (length x) 1) - should NOT be optimized
        let mut expr = Expr::Call {
            func: Box::new(Expr::GlobalVar(eq_sym)),
            args: vec![
                Expr::Call {
                    func: Box::new(Expr::GlobalVar(length_sym)),
                    args: vec![Expr::GlobalVar(x_sym)],
                    tail: false,
                },
                Expr::Literal(Value::Int(1)),
            ],
            tail: false,
        };

        let original = expr.clone();
        optimize(&mut expr, &symbols);

        // Should NOT be transformed
        assert_eq!(expr, original);
    }
}
