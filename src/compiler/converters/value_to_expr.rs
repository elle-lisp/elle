use super::super::ast::Expr;
use super::super::macros::expand_macro;
use super::binding_forms::{convert_lambda, convert_let, convert_let_star, convert_letrec};
use super::control_flow::{convert_cond, convert_match_expr};
use super::exception_handling::{convert_handler_bind, convert_handler_case, convert_try};
use super::quasiquote::expand_quasiquote;
use super::threading::{handle_thread_first, handle_thread_last};
use super::{ScopeEntry, ScopeType};
use crate::binding::VarRef;
use crate::symbol::SymbolTable;
use crate::value::Value;
use crate::value_old::SymbolId;

/// Simple value-to-expr conversion for bootstrap
/// This is a simple tree-walking approach before full macro expansion
pub fn value_to_expr(value: &Value, symbols: &mut SymbolTable) -> Result<Expr, String> {
    let mut scope_stack: Vec<ScopeEntry> = Vec::new();
    let mut expr = value_to_expr_with_scope(value, symbols, &mut scope_stack)?;
    super::super::capture_resolution::resolve_captures(&mut expr);
    mark_tail_calls(&mut expr, true);
    super::super::optimize::optimize(&mut expr, symbols);
    Ok(expr)
}

/// Convert a value to an expression, tracking local variable scopes
/// The scope_stack contains local bindings (as ScopeEntry for ordering) at each nesting level
pub fn value_to_expr_with_scope(
    value: &Value,
    symbols: &mut SymbolTable,
    scope_stack: &mut Vec<ScopeEntry>,
) -> Result<Expr, String> {
    // Handle literal values
    if value.is_nil()
        || value.is_bool()
        || value.is_int()
        || value.is_float()
        || value.is_string()
        || value.is_keyword()
        || value.is_vector()
        || value.is_empty_list()
    {
        return Ok(Expr::Literal(*value));
    }

    // Handle symbols
    if let Some(id) = value.as_symbol() {
        let id = SymbolId(id);
        // Check if the symbol is a local binding by walking up the scope stack
        for (idx, scope_entry) in scope_stack.iter().enumerate().rev() {
            if let Some(local_index) = scope_entry.symbols.iter().position(|sym| sym == &id) {
                let depth = scope_stack.len() - 1 - idx;

                if scope_entry.scope_type == ScopeType::Let {
                    // Let-bound variable
                    let mut has_intervening_lambda = false;
                    for scope in scope_stack.iter().skip(idx + 1) {
                        if scope.scope_type == ScopeType::Function {
                            has_intervening_lambda = true;
                            break;
                        }
                    }

                    if has_intervening_lambda {
                        // Inside a lambda that needs to capture this variable
                        return Ok(Expr::Var(VarRef::upvalue(id, local_index, false)));
                    } else {
                        // Direct access to let-bound variable - use symbol for scope stack lookup
                        return Ok(Expr::Var(VarRef::let_bound(id)));
                    }
                } else {
                    // Lambda parameter or capture
                    if depth == 0 {
                        // Current lambda's scope
                        return Ok(Expr::Var(VarRef::local(local_index)));
                    } else {
                        // Outer lambda's scope - needs capture
                        return Ok(Expr::Var(VarRef::upvalue(id, local_index, false)));
                    }
                }
            }
        }
        // Not found in any local scope - treat as global
        return Ok(Expr::Var(VarRef::global(id)));
    }

    // Handle cons cells
    if value.as_cons().is_some() {
        let list = value.list_to_vec()?;
        if list.is_empty() {
            return Err("Empty list in expression".to_string());
        }

        let first = &list[0];
        if let Some(sym) = first.as_symbol() {
            let sym_id = SymbolId(sym);
            let name = symbols.name(sym_id).ok_or("Unknown symbol")?;

            match name {
                "qualified-ref" => {
                    // Handle module-qualified symbols: (qualified-ref module-name symbol-name)
                    if list.len() != 3 {
                        return Err("qualified-ref requires exactly 2 arguments".to_string());
                    }
                    let module_sym = list[1].as_symbol().ok_or("Expected symbol")?;
                    let name_sym = list[2].as_symbol().ok_or("Expected symbol")?;
                    let module_sym_id = SymbolId(module_sym);
                    let name_sym_id = SymbolId(name_sym);

                    let module_name = symbols.name(module_sym_id).ok_or("Unknown module symbol")?;
                    let func_name = symbols.name(name_sym_id).ok_or("Unknown function symbol")?;

                    // Try to resolve from the specified module's exports
                    if let Some(module_def) = symbols.get_module(module_sym_id) {
                        // Check if the symbol is exported from the module
                        if module_def.exports.contains(&name_sym_id) {
                            // Return as a qualified global reference
                            Ok(Expr::Var(VarRef::global(name_sym_id)))
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
                    Ok(Expr::Literal(list[1]))
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
                    let cond = Box::new(value_to_expr_with_scope(&list[1], symbols, scope_stack)?);
                    let then = Box::new(value_to_expr_with_scope(&list[2], symbols, scope_stack)?);
                    let else_ = if list.len() == 4 {
                        Box::new(value_to_expr_with_scope(&list[3], symbols, scope_stack)?)
                    } else {
                        Box::new(Expr::Literal(Value::NIL))
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
                    let name = SymbolId(list[1].as_symbol().ok_or("Expected symbol")?);

                    // Register the variable in the current scope BEFORE processing the value
                    // This way, if the value references the variable (unusual but possible),
                    // it can be found. More importantly, it makes the variable available to
                    // subsequent expressions in the same scope.
                    if !scope_stack.is_empty() {
                        let scope_entry = scope_stack.last_mut().unwrap();
                        if !scope_entry.symbols.contains(&name) {
                            scope_entry.symbols.push(name);
                        }
                    }

                    let value = Box::new(value_to_expr_with_scope(&list[2], symbols, scope_stack)?);

                    Ok(Expr::Define { name, value })
                }

                "let" => convert_let(&list, symbols, scope_stack),

                "let*" => convert_let_star(&list, symbols, scope_stack),

                "letrec" => convert_letrec(&list, symbols, scope_stack),

                "set!" => {
                    if list.len() != 3 {
                        return Err("set! requires exactly 2 arguments".to_string());
                    }
                    let var = list[1].as_symbol().ok_or("Expected symbol")?;
                    let value = Box::new(value_to_expr_with_scope(&list[2], symbols, scope_stack)?);

                    // Look up the variable in the scope stack to create VarRef
                    let target = lookup_var_for_set(var, scope_stack);

                    Ok(Expr::Set { target, value })
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
                    let func_sym = first.as_symbol().ok_or("Expected symbol")?;
                    let func = Box::new(Expr::Var(VarRef::global(SymbolId(func_sym))));
                    let args = vec![value_to_expr_with_scope(&list[1], symbols, scope_stack)?];
                    Ok(Expr::Call {
                        func,
                        args,
                        tail: false,
                    })
                }

                "yield" => {
                    // Syntax: (yield <value>)
                    // Yield suspends coroutine execution and returns a value
                    if list.len() != 2 {
                        return Err("yield requires exactly one argument".to_string());
                    }
                    let expr = value_to_expr_with_scope(&list[1], symbols, scope_stack)?;
                    Ok(Expr::Yield(Box::new(expr)))
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
                    let name = SymbolId(list[1].as_symbol().ok_or("Expected symbol")?);
                    let params_val = &list[2];

                    // Parse parameter list
                    let params = if params_val.is_list() {
                        let param_vec = params_val.list_to_vec()?;
                        param_vec
                            .iter()
                            .map(|v| v.as_symbol().map(SymbolId))
                            .collect::<Option<Vec<_>>>()
                            .ok_or("Macro parameters must be symbols")?
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
                    let body = Box::new(Expr::Literal(Value::NIL));

                    Ok(Expr::DefMacro { name, params, body })
                }

                "while" => {
                    // Syntax: (while condition body)
                    if list.len() != 3 {
                        return Err(
                            "while requires exactly 2 arguments (condition body)".to_string()
                        );
                    }
                    let cond = Box::new(value_to_expr_with_scope(&list[1], symbols, scope_stack)?);
                    let body = Box::new(value_to_expr_with_scope(&list[2], symbols, scope_stack)?);
                    Ok(Expr::While { cond, body })
                }

                "forever" => {
                    // Syntax: (forever body...)
                    // Expands to: (while #t body...)
                    if list.len() < 2 {
                        return Err("forever requires at least 1 argument (body)".to_string());
                    }
                    // Combine all body expressions into a single expression
                    let body = if list.len() == 2 {
                        // Single body expression
                        Box::new(value_to_expr_with_scope(&list[1], symbols, scope_stack)?)
                    } else {
                        // Multiple body expressions - wrap in begin
                        let body_exprs: Result<Vec<Expr>, String> = list[1..]
                            .iter()
                            .map(|expr| value_to_expr_with_scope(expr, symbols, scope_stack))
                            .collect();
                        Box::new(Expr::Begin(body_exprs?))
                    };
                    // Create a while loop with #t as the condition
                    let cond = Box::new(Expr::Literal(Value::TRUE));
                    Ok(Expr::While { cond, body })
                }

                "each" => {
                    // Syntax: (each var iter body)
                    // Also supports: (each var in iter body) for clarity
                    if list.len() < 4 || list.len() > 5 {
                        return Err(
                            "each requires 3 or 4 arguments (var [in] iter body)".to_string()
                        );
                    }

                    let var = SymbolId(list[1].as_symbol().ok_or("Expected symbol")?);
                    let (iter_expr, body_expr) = if list.len() == 4 {
                        // (each var iter body)
                        (&list[2], &list[3])
                    } else {
                        // (each var in iter body)
                        if let Some(in_sym) = list[2].as_symbol() {
                            let in_sym_id = SymbolId(in_sym);
                            if let Some("in") = symbols.name(in_sym_id) {
                                (&list[3], &list[4])
                            } else {
                                return Err("each loop syntax: (each var iter body) or (each var in iter body)".to_string());
                            }
                        } else {
                            return Err(
                                "each loop syntax: (each var iter body) or (each var in iter body)"
                                    .to_string(),
                            );
                        }
                    };

                    // Compile iterator expression
                    let iter = Box::new(value_to_expr_with_scope(iter_expr, symbols, scope_stack)?);

                    // Compile body expression (the loop variable will be set as a global at runtime)
                    // Note: the loop variable is accessible in the body as a global
                    let body = Box::new(value_to_expr_with_scope(body_expr, symbols, scope_stack)?);

                    Ok(Expr::For { var, iter, body })
                }

                "and" => {
                    // Syntax: (and expr1 expr2 ...)
                    // Short-circuit evaluation: returns first falsy value or last value
                    if list.len() < 2 {
                        return Ok(Expr::Literal(Value::TRUE)); // (and) => true
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
                        return Ok(Expr::Literal(Value::FALSE)); // (or) => false
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
                        return Ok(Expr::Literal(Value::FALSE)); // (xor) => false
                    }

                    let func = Box::new(Expr::Var(VarRef::global(symbols.intern("xor"))));
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
                    if let Some(sym_id_u32) = first.as_symbol() {
                        let sym_id = SymbolId(sym_id_u32);
                        if symbols.is_macro(sym_id) {
                            if let Some(macro_def) = symbols.get_macro(sym_id) {
                                // This is a macro call - expand it
                                // Get the arguments as unevaluated values
                                let args = list[1..].to_vec();

                                // Expand the macro
                                let expanded = expand_macro(sym_id, &macro_def, &args, symbols)?;

                                // Parse the expanded result as an expression
                                return value_to_expr_with_scope(&expanded, symbols, scope_stack);
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
    } else {
        // Not a cons cell - error
        Err(format!("Cannot convert {:?} to expression", value))
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

/// Look up a variable for set! and return the appropriate VarRef
fn lookup_var_for_set(var: u32, scope_stack: &[ScopeEntry]) -> VarRef {
    let var_id = SymbolId(var);

    for (idx, scope_entry) in scope_stack.iter().enumerate().rev() {
        if let Some(local_index) = scope_entry.symbols.iter().position(|sym| sym == &var_id) {
            if scope_entry.scope_type == ScopeType::Let {
                // Let-bound variable - check for intervening lambda
                let mut has_intervening_lambda = false;
                for scope in scope_stack.iter().skip(idx + 1) {
                    if scope.scope_type == ScopeType::Function {
                        has_intervening_lambda = true;
                        break;
                    }
                }

                if has_intervening_lambda {
                    // Inside a lambda that captures this variable
                    return VarRef::upvalue(var_id, local_index, false);
                } else {
                    // Direct access to let-bound variable
                    return VarRef::let_bound(var_id);
                }
            } else {
                // Function scope variable
                let depth = scope_stack.len() - 1 - idx;
                if depth == 0 {
                    return VarRef::local(local_index);
                } else {
                    return VarRef::upvalue(var_id, local_index, false);
                }
            }
        }
    }
    // Not found in local scopes - it's global
    VarRef::global(var_id)
}

/// Convert a value back to source code using the symbol table
fn value_to_source(value: &Value, symbols: &SymbolTable) -> String {
    if value.is_nil() {
        "nil".to_string()
    } else if value.is_empty_list() {
        "()".to_string()
    } else if let Some(b) = value.as_bool() {
        b.to_string()
    } else if let Some(n) = value.as_int() {
        n.to_string()
    } else if let Some(f) = value.as_float() {
        f.to_string()
    } else if let Some(id) = value.as_symbol() {
        // Look up the symbol name in the symbol table
        let sym_id = SymbolId(id);
        symbols
            .name(sym_id)
            .unwrap_or(&format!("Symbol({})", id))
            .to_string()
    } else if let Some(id) = value.as_keyword() {
        format!(":{}", id)
    } else if let Some(s) = value.as_string() {
        format!("\"{}\"", s)
    } else if value.as_cons().is_some() {
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
    } else if let Some(vec) = value.as_vector() {
        let items: Vec<String> = vec
            .borrow()
            .iter()
            .map(|v| value_to_source(v, symbols))
            .collect();
        format!("[{}]", items.join(" "))
    } else {
        // For other types, use debug representation
        format!("{:?}", value)
    }
}
