use super::super::ast::Expr;
use crate::symbol::SymbolTable;
use crate::value::{SymbolId, Value};

/// Helper function to convert handler-case expressions
/// Extracted to reduce stack frame size of value_to_expr_with_scope
#[inline(never)]
pub fn convert_handler_case(
    list: &[Value],
    symbols: &mut SymbolTable,
    scope_stack: &mut Vec<Vec<SymbolId>>,
) -> Result<Expr, String> {
    use super::value_to_expr::value_to_expr_with_scope;

    // Syntax: (handler-case body (exception-id (var) handler-code) ...)
    // exception-id can be numeric or a symbol (will be looked up)
    if list.len() < 2 {
        return Err("handler-case requires at least a body".to_string());
    }

    let body = Box::new(value_to_expr_with_scope(&list[1], symbols, scope_stack)?);
    let mut handlers = Vec::new();

    // Parse handler clauses
    for clause in &list[2..] {
        if let Ok(clause_vec) = clause.list_to_vec() {
            if clause_vec.len() != 3 {
                return Err("handler-case clause requires (exception-id (var) handler)".to_string());
            }

            // Parse exception ID
            let exception_id = match &clause_vec[0] {
                Value::Int(id) => *id as u32,
                Value::Symbol(sym) => {
                    // Map symbol to exception ID
                    let name = symbols.name(*sym).unwrap_or("unknown");
                    match name {
                        "condition" => 1,
                        "error" => 2,
                        "type-error" => 3,
                        "division-by-zero" => 4,
                        "undefined-variable" => 5,
                        "arity-error" => 6,
                        "warning" => 7,
                        "style-warning" => 8,
                        _ => return Err(format!("Unknown exception type: {}", name)),
                    }
                }
                _ => return Err("Exception ID must be integer or symbol".to_string()),
            };

            // Parse variable
            let var = clause_vec[1].as_symbol()?;

            // Parse handler code
            let handler_code = Box::new(value_to_expr_with_scope(
                &clause_vec[2],
                symbols,
                scope_stack,
            )?);

            handlers.push((exception_id, var, handler_code));
        } else {
            return Err("Handler clauses must be lists".to_string());
        }
    }

    Ok(Expr::HandlerCase { body, handlers })
}

/// Helper function to convert handler-bind expressions
/// Extracted to reduce stack frame size of value_to_expr_with_scope
#[inline(never)]
pub fn convert_handler_bind(
    list: &[Value],
    symbols: &mut SymbolTable,
    scope_stack: &mut Vec<Vec<SymbolId>>,
) -> Result<Expr, String> {
    use super::value_to_expr::value_to_expr_with_scope;

    // Syntax: (handler-bind ((exception-id handler-fn) ...) body)
    // handler-fn is called but doesn't unwind the stack
    if list.len() != 3 {
        return Err("handler-bind requires ((handlers...) body)".to_string());
    }

    let handlers_list = list[1].list_to_vec()?;
    let mut handlers = Vec::new();

    for handler_spec in handlers_list {
        let spec_vec = handler_spec.list_to_vec()?;
        if spec_vec.len() != 2 {
            return Err("Each handler binding must be (exception-id handler-fn)".to_string());
        }

        // Parse exception ID
        let exception_id = match &spec_vec[0] {
            Value::Int(id) => *id as u32,
            Value::Symbol(sym) => {
                let name = symbols.name(*sym).unwrap_or("unknown");
                match name {
                    "condition" => 1,
                    "error" => 2,
                    "type-error" => 3,
                    "division-by-zero" => 4,
                    "undefined-variable" => 5,
                    "arity-error" => 6,
                    "warning" => 7,
                    "style-warning" => 8,
                    _ => return Err(format!("Unknown exception type: {}", name)),
                }
            }
            _ => return Err("Exception ID must be integer or symbol".to_string()),
        };

        // Parse handler function
        let handler_fn = Box::new(value_to_expr_with_scope(
            &spec_vec[1],
            symbols,
            scope_stack,
        )?);

        handlers.push((exception_id, handler_fn));
    }

    let body = Box::new(value_to_expr_with_scope(&list[2], symbols, scope_stack)?);

    Ok(Expr::HandlerBind { handlers, body })
}

/// Convert try-catch-finally expressions
/// Syntax: `(try body (catch var handler) (finally expr)?)`
#[inline(never)]
pub fn convert_try(
    list: &[Value],
    symbols: &mut SymbolTable,
    scope_stack: &mut Vec<Vec<SymbolId>>,
) -> Result<Expr, String> {
    use super::value_to_expr::value_to_expr_with_scope;

    if list.len() < 2 {
        return Err("try requires at least a body".to_string());
    }

    let body = Box::new(value_to_expr_with_scope(&list[1], symbols, scope_stack)?);
    let mut catch_clause = None;
    let mut finally_clause = None;

    // Parse catch and finally clauses
    for item in &list[2..] {
        if item.is_list() {
            let v = item.list_to_vec()?;
            if v.is_empty() {
                return Err("Empty clause in try expression".to_string());
            }
            if let Value::Symbol(keyword) = &v[0] {
                let keyword_str = symbols.name(*keyword).unwrap_or("unknown");
                match keyword_str {
                    "catch" => {
                        if v.len() != 3 {
                            return Err(
                                "catch requires exactly 2 arguments (variable and handler)"
                                    .to_string(),
                            );
                        }
                        let var = v[1].as_symbol()?;
                        let handler =
                            Box::new(value_to_expr_with_scope(&v[2], symbols, scope_stack)?);
                        catch_clause = Some((var, handler));
                    }
                    "finally" => {
                        if v.len() != 2 {
                            return Err("finally requires exactly 1 argument".to_string());
                        }
                        finally_clause = Some(Box::new(value_to_expr_with_scope(
                            &v[1],
                            symbols,
                            scope_stack,
                        )?));
                    }
                    _ => {
                        return Err(format!("Unknown clause in try: {}", keyword_str));
                    }
                }
            } else {
                return Err("Clause keyword must be a symbol".to_string());
            }
        } else {
            return Err("Clauses in try must be lists".to_string());
        }
    }

    Ok(Expr::Try {
        body,
        catch: catch_clause,
        finally: finally_clause,
    })
}
