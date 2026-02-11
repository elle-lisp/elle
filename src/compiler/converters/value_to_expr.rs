use super::super::ast::Expr;
use super::super::macros::expand_macro;
use super::binding_forms::{convert_lambda, convert_let, convert_let_star, convert_letrec};
use super::control_flow::{convert_cond, convert_match_expr};
use super::exception_handling::{convert_handler_bind, convert_handler_case, convert_try};
use super::quasiquote::expand_quasiquote;
use super::threading::{handle_thread_first, handle_thread_last};
use crate::symbol::SymbolTable;
use crate::value::{SymbolId, Value};

/// Simple value-to-expr conversion for bootstrap
/// This is a simple tree-walking approach before full macro expansion
pub fn value_to_expr(value: &Value, symbols: &mut SymbolTable) -> Result<Expr, String> {
    let mut expr = value_to_expr_with_scope(value, symbols, &mut Vec::new())?;
    super::super::capture_resolution::resolve_captures(&mut expr);
    mark_tail_calls(&mut expr, true);
    Ok(expr)
}

/// Convert a value to an expression, tracking local variable scopes
/// The scope_stack contains local bindings (as Vec for ordering) at each nesting level
pub fn value_to_expr_with_scope(
    value: &Value,
    symbols: &mut SymbolTable,
    scope_stack: &mut Vec<Vec<SymbolId>>,
) -> Result<Expr, String> {
    match value {
        Value::Nil
        | Value::Bool(_)
        | Value::Int(_)
        | Value::Float(_)
        | Value::String(_)
        | Value::Keyword(_)
        | Value::Vector(_) => Ok(Expr::Literal(value.clone())),

        Value::Symbol(id) => {
            // Check if the symbol is a local binding by walking up the scope stack
            for (reverse_idx, scope) in scope_stack.iter().enumerate().rev() {
                if let Some(local_index) = scope.iter().position(|sym| sym == id) {
                    // Found in local scope - use Var with appropriate depth and index
                    // depth represents how many function scopes up the variable is defined:
                    // 0 = current lambda's parameters
                    // 1 = enclosing lambda's parameters
                    // etc.
                    let actual_depth = scope_stack.len() - 1 - reverse_idx;
                    return Ok(Expr::Var(*id, actual_depth, local_index));
                }
            }
            // Not found in any local scope - treat as global
            Ok(Expr::GlobalVar(*id))
        }

        Value::Cons(_) => {
            let list = value.list_to_vec()?;
            if list.is_empty() {
                return Err("Empty list in expression".to_string());
            }

            let first = &list[0];
            if let Value::Symbol(sym) = first {
                let name = symbols.name(*sym).ok_or("Unknown symbol")?;

                match name {
                    "qualified-ref" => {
                        // Handle module-qualified symbols: (qualified-ref module-name symbol-name)
                        if list.len() != 3 {
                            return Err("qualified-ref requires exactly 2 arguments".to_string());
                        }
                        let module_sym = list[1].as_symbol()?;
                        let name_sym = list[2].as_symbol()?;

                        let module_name =
                            symbols.name(module_sym).ok_or("Unknown module symbol")?;
                        let func_name = symbols.name(name_sym).ok_or("Unknown function symbol")?;

                        // Try to resolve from the specified module's exports
                        if let Some(module_def) = symbols.get_module(module_sym) {
                            // Check if the symbol is exported from the module
                            if module_def.exports.contains(&name_sym) {
                                // Return as a qualified global reference
                                // We use GlobalVar but could add a QualifiedVar variant if needed
                                Ok(Expr::GlobalVar(name_sym))
                            } else {
                                Err(format!(
                                    "Symbol '{}' not exported from module '{}'",
                                    func_name, module_name
                                ))
                            }
                        } else {
                            Err(format!("Unknown module: '{}'", module_name))
                        }
                    }

                    "quote" => {
                        if list.len() != 2 {
                            return Err("quote requires exactly 1 argument".to_string());
                        }
                        Ok(Expr::Literal(list[1].clone()))
                    }

                    "quasiquote" => {
                        if list.len() != 2 {
                            return Err("quasiquote requires exactly 1 argument".to_string());
                        }
                        // Convert the quasiquote form into a proper Expr::Quasiquote
                        // The content is processed to handle unquotes
                        let content = &list[1];
                        expand_quasiquote(content, symbols, scope_stack)
                    }

                    "unquote" | "unquote-splicing" => {
                        // Unquote outside of quasiquote is an error
                        Err(format!("{} can only be used inside quasiquote", name))
                    }

                    "if" => {
                        if list.len() < 3 || list.len() > 4 {
                            return Err("if requires 2 or 3 arguments".to_string());
                        }
                        let cond =
                            Box::new(value_to_expr_with_scope(&list[1], symbols, scope_stack)?);
                        let then =
                            Box::new(value_to_expr_with_scope(&list[2], symbols, scope_stack)?);
                        let else_ = if list.len() == 4 {
                            Box::new(value_to_expr_with_scope(&list[3], symbols, scope_stack)?)
                        } else {
                            Box::new(Expr::Literal(Value::Nil))
                        };
                        Ok(Expr::If { cond, then, else_ })
                    }

                    "begin" => {
                        // Process expressions sequentially to handle variable definitions properly
                        // This allows define to register variables that are then available to later expressions
                        let mut exprs = Vec::new();
                        for v in &list[1..] {
                            let expr = value_to_expr_with_scope(v, symbols, scope_stack)?;
                            exprs.push(expr);
                        }
                        Ok(Expr::Begin(exprs))
                    }

                    "block" => {
                        let exprs: Result<Vec<_>, _> = list[1..]
                            .iter()
                            .map(|v| value_to_expr_with_scope(v, symbols, scope_stack))
                            .collect();
                        Ok(Expr::Block(exprs?))
                    }

                    "fn" | "lambda" => convert_lambda(&list, symbols, scope_stack),

                    "define" => {
                        if list.len() != 3 {
                            return Err("define requires exactly 2 arguments".to_string());
                        }
                        let name = list[1].as_symbol()?;

                        // Register the variable in the current scope BEFORE processing the value
                        // This way, if the value references the variable (unusual but possible),
                        // it can be found. More importantly, it makes the variable available to
                        // subsequent expressions in the same scope.
                        if !scope_stack.is_empty() {
                            let scope = scope_stack.last_mut().unwrap();
                            if !scope.contains(&name) {
                                scope.push(name);
                            }
                        }

                        let value =
                            Box::new(value_to_expr_with_scope(&list[2], symbols, scope_stack)?);

                        Ok(Expr::Define { name, value })
                    }

                    "let" => convert_let(&list, symbols, scope_stack),

                    "let*" => convert_let_star(&list, symbols, scope_stack),

                    "letrec" => convert_letrec(&list, symbols, scope_stack),

                    "set!" => {
                        if list.len() != 3 {
                            return Err("set! requires exactly 2 arguments".to_string());
                        }
                        let var = list[1].as_symbol()?;
                        let value =
                            Box::new(value_to_expr_with_scope(&list[2], symbols, scope_stack)?);

                        // Look up the variable in the scope stack to determine depth and index
                        let mut depth = 0;
                        let mut index = usize::MAX; // Use MAX to signal global variable

                        for (reverse_idx, scope) in scope_stack.iter().enumerate().rev() {
                            if let Some(local_index) = scope.iter().position(|sym| sym == &var) {
                                depth = scope_stack.len() - 1 - reverse_idx;
                                index = local_index;
                                break;
                            }
                        }

                        // If not found in local scopes (index == usize::MAX), it's a global variable set

                        Ok(Expr::Set {
                            var,
                            depth,
                            index,
                            value,
                        })
                    }

                    "try" => convert_try(&list, symbols, scope_stack),

                    "handler-case" => convert_handler_case(&list, symbols, scope_stack),

                    "handler-bind" => convert_handler_bind(&list, symbols, scope_stack),

                    "match" => convert_match_expr(&list, symbols, scope_stack),

                    "throw" => {
                        // Syntax: (throw <exception>)
                        // Throw is a special form that compiles to a function call
                        // The throw primitive will convert the exception to a Rust error
                        if list.len() != 2 {
                            return Err("throw requires exactly 1 argument".to_string());
                        }
                        // Compile as a regular function call to the throw primitive
                        let func = Box::new(Expr::GlobalVar(first.as_symbol()?));
                        let args = vec![value_to_expr_with_scope(&list[1], symbols, scope_stack)?];
                        Ok(Expr::Call {
                            func,
                            args,
                            tail: false,
                        })
                    }

                    "defmacro" | "define-macro" => {
                        // Syntax: (defmacro name (params...) body)
                        //      or (define-macro name (params...) body)
                        if list.len() != 4 {
                            return Err(
                                "defmacro requires exactly 3 arguments (name, parameters, body)"
                                    .to_string(),
                            );
                        }
                        let name = list[1].as_symbol()?;
                        let params_val = &list[2];

                        // Parse parameter list
                        let params = if params_val.is_list() {
                            let param_vec = params_val.list_to_vec()?;
                            param_vec
                                .iter()
                                .map(|v| v.as_symbol())
                                .collect::<Result<Vec<_>, _>>()?
                        } else {
                            return Err("Macro parameters must be a list".to_string());
                        };

                        // Store macro body as source code for later expansion
                        // Convert the value back to source code using the symbol table
                        let body_str = value_to_source(&list[3], symbols);

                        // Register the macro in the symbol table
                        use crate::symbol::MacroDef;
                        symbols.define_macro(MacroDef {
                            name,
                            params: params.clone(),
                            body: body_str,
                        });

                        // Don't compile the macro body - it will be expanded at call time
                        // The body is stored as source code in the MacroDef
                        let body = Box::new(Expr::Literal(Value::Nil));

                        Ok(Expr::DefMacro { name, params, body })
                    }

                    "while" => {
                        // Syntax: (while condition body)
                        if list.len() != 3 {
                            return Err(
                                "while requires exactly 2 arguments (condition body)".to_string()
                            );
                        }
                        let cond =
                            Box::new(value_to_expr_with_scope(&list[1], symbols, scope_stack)?);
                        let body =
                            Box::new(value_to_expr_with_scope(&list[2], symbols, scope_stack)?);
                        Ok(Expr::While { cond, body })
                    }

                    "for" => {
                        // Syntax: (for var iter body)
                        // Also supports: (for var in iter body) for clarity
                        if list.len() < 4 || list.len() > 5 {
                            return Err(
                                "for requires 3 or 4 arguments (var [in] iter body)".to_string()
                            );
                        }

                        let var = list[1].as_symbol()?;
                        let (iter_expr, body_expr) = if list.len() == 4 {
                            // (for var iter body)
                            (&list[2], &list[3])
                        } else {
                            // (for var in iter body)
                            if let Value::Symbol(in_sym) = &list[2] {
                                if let Some("in") = symbols.name(*in_sym) {
                                    (&list[3], &list[4])
                                } else {
                                    return Err("for loop syntax: (for var iter body) or (for var in iter body)".to_string());
                                }
                            } else {
                                return Err("for loop syntax: (for var iter body) or (for var in iter body)".to_string());
                            }
                        };

                        // Compile iterator expression
                        let iter =
                            Box::new(value_to_expr_with_scope(iter_expr, symbols, scope_stack)?);

                        // Compile body expression (the loop variable will be set as a global at runtime)
                        // Note: the loop variable is accessible in the body as a global
                        let body =
                            Box::new(value_to_expr_with_scope(body_expr, symbols, scope_stack)?);

                        Ok(Expr::For { var, iter, body })
                    }

                    "and" => {
                        // Syntax: (and expr1 expr2 ...)
                        // Short-circuit evaluation: returns first falsy value or last value
                        if list.len() < 2 {
                            return Ok(Expr::Literal(Value::Bool(true))); // (and) => true
                        }
                        let exprs: Result<Vec<_>, _> = list[1..]
                            .iter()
                            .map(|v| value_to_expr_with_scope(v, symbols, scope_stack))
                            .collect();
                        Ok(Expr::And(exprs?))
                    }

                    "or" => {
                        // Syntax: (or expr1 expr2 ...)
                        // Short-circuit evaluation: returns first truthy value or last value
                        if list.len() < 2 {
                            return Ok(Expr::Literal(Value::Bool(false))); // (or) => false
                        }
                        let exprs: Result<Vec<_>, _> = list[1..]
                            .iter()
                            .map(|v| value_to_expr_with_scope(v, symbols, scope_stack))
                            .collect();
                        Ok(Expr::Or(exprs?))
                    }

                    "xor" => {
                        // Syntax: (xor expr1 expr2 ...)
                        // Transform to a function call to the xor primitive
                        // This way we don't need special compilation logic
                        if list.len() < 2 {
                            return Ok(Expr::Literal(Value::Bool(false))); // (xor) => false
                        }

                        let func = Box::new(Expr::GlobalVar(symbols.intern("xor")));
                        let args: Result<Vec<_>, _> = list[1..]
                            .iter()
                            .map(|v| value_to_expr_with_scope(v, symbols, scope_stack))
                            .collect();
                        Ok(Expr::Call {
                            func,
                            args: args?,
                            tail: false,
                        })
                    }

                    "->" => handle_thread_first(&list, symbols, scope_stack),

                    "->>" => handle_thread_last(&list, symbols, scope_stack),

                    "cond" => convert_cond(&list, symbols, scope_stack),

                    _ => {
                        // Check if it's a macro call
                        if let Value::Symbol(sym_id) = first {
                            if symbols.is_macro(*sym_id) {
                                if let Some(macro_def) = symbols.get_macro(*sym_id) {
                                    // This is a macro call - expand it
                                    // Get the arguments as unevaluated values
                                    let args = list[1..].to_vec();

                                    // Expand the macro
                                    let expanded =
                                        expand_macro(*sym_id, &macro_def, &args, symbols)?;

                                    // Parse the expanded result as an expression
                                    return value_to_expr_with_scope(
                                        &expanded,
                                        symbols,
                                        scope_stack,
                                    );
                                }
                            }
                        }

                        // Regular function call
                        let func = Box::new(value_to_expr_with_scope(first, symbols, scope_stack)?);
                        let args: Result<Vec<_>, _> = list[1..]
                            .iter()
                            .map(|v| value_to_expr_with_scope(v, symbols, scope_stack))
                            .collect();
                        Ok(Expr::Call {
                            func,
                            args: args?,
                            tail: false,
                        })
                    }
                }
            } else {
                // Function call with non-symbol function
                let func = Box::new(value_to_expr_with_scope(first, symbols, scope_stack)?);
                let args: Result<Vec<_>, _> = list[1..]
                    .iter()
                    .map(|v| value_to_expr_with_scope(v, symbols, scope_stack))
                    .collect();
                Ok(Expr::Call {
                    func,
                    args: args?,
                    tail: false,
                })
            }
        }

        _ => Err(format!("Cannot convert {:?} to expression", value)),
    }
}

/// Mark Call expressions in tail position with tail=true for TCO
pub fn mark_tail_calls(expr: &mut Expr, in_tail: bool) {
    match expr {
        Expr::Call { func, args, tail } => {
            if in_tail {
                *tail = true;
            }
            // Function and arguments are NOT in tail position
            mark_tail_calls(func, false);
            for arg in args {
                mark_tail_calls(arg, false);
            }
        }
        Expr::If { cond, then, else_ } => {
            mark_tail_calls(cond, false);
            mark_tail_calls(then, in_tail);
            mark_tail_calls(else_, in_tail);
        }
        Expr::Cond { clauses, else_body } => {
            for (test, body) in clauses {
                mark_tail_calls(test, false);
                mark_tail_calls(body, in_tail);
            }
            if let Some(else_expr) = else_body {
                mark_tail_calls(else_expr, in_tail);
            }
        }
        Expr::Begin(exprs) => {
            let len = exprs.len();
            for (i, e) in exprs.iter_mut().enumerate() {
                let is_last = i == len - 1;
                mark_tail_calls(e, in_tail && is_last);
            }
        }
        Expr::Block(exprs) => {
            let len = exprs.len();
            for (i, e) in exprs.iter_mut().enumerate() {
                let is_last = i == len - 1;
                mark_tail_calls(e, in_tail && is_last);
            }
        }
        Expr::Lambda { body, .. } => {
            // Lambda body is in tail position (of the lambda)
            mark_tail_calls(body, true);
        }
        Expr::Let { bindings, body } => {
            for (_, e) in bindings {
                mark_tail_calls(e, false);
            }
            mark_tail_calls(body, in_tail);
        }
        Expr::Letrec { bindings, body } => {
            for (_, e) in bindings {
                mark_tail_calls(e, false);
            }
            mark_tail_calls(body, in_tail);
        }
        Expr::Set { value, .. } => {
            mark_tail_calls(value, false);
        }
        Expr::Define { value, .. } => {
            mark_tail_calls(value, false);
        }
        Expr::While { cond, body } => {
            mark_tail_calls(cond, false);
            mark_tail_calls(body, false); // Loop body is not in tail position
        }
        Expr::For { iter, body, .. } => {
            mark_tail_calls(iter, false);
            mark_tail_calls(body, false);
        }
        Expr::Match {
            value,
            patterns,
            default,
        } => {
            mark_tail_calls(value, false);
            for (_, body) in patterns {
                mark_tail_calls(body, in_tail);
            }
            if let Some(d) = default {
                mark_tail_calls(d, in_tail);
            }
        }
        Expr::Try {
            body,
            catch,
            finally,
        } => {
            // Try body is NOT in tail position (might need to run finally)
            mark_tail_calls(body, false);
            if let Some((_, handler)) = catch {
                mark_tail_calls(handler, false);
            }
            if let Some(f) = finally {
                mark_tail_calls(f, false);
            }
        }
        Expr::And(exprs) | Expr::Or(exprs) => {
            // Only the last expression in and/or is in tail position
            let len = exprs.len();
            for (i, e) in exprs.iter_mut().enumerate() {
                let is_last = i == len - 1;
                mark_tail_calls(e, in_tail && is_last);
            }
        }
        Expr::DefMacro { body, .. } => {
            mark_tail_calls(body, false);
        }
        // Leaf nodes and others — nothing to do
        _ => {}
    }
}

/// Convert a value back to source code using the symbol table
fn value_to_source(value: &Value, symbols: &SymbolTable) -> String {
    match value {
        Value::Nil => "nil".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Int(n) => n.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Symbol(id) => {
            // Look up the symbol name in the symbol table
            symbols
                .name(*id)
                .unwrap_or(&format!("Symbol({})", id.0))
                .to_string()
        }
        Value::Keyword(id) => {
            format!(":{}", id.0)
        }
        Value::String(s) => {
            format!("\"{}\"", s)
        }
        Value::Cons(_) => {
            // Convert list to source code
            if let Ok(list_vec) = value.list_to_vec() {
                let items: Vec<String> = list_vec
                    .iter()
                    .map(|v| value_to_source(v, symbols))
                    .collect();
                format!("({})", items.join(" "))
            } else {
                // Improper list
                format!("{:?}", value)
            }
        }
        Value::Vector(vec) => {
            let items: Vec<String> = vec.iter().map(|v| value_to_source(v, symbols)).collect();
            format!("[{}]", items.join(" "))
        }
        _ => {
            // For other types, use debug representation
            format!("{:?}", value)
        }
    }
}
