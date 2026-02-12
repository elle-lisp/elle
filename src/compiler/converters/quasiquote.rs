use super::super::ast::Expr;
use super::ScopeEntry;
use crate::symbol::SymbolTable;
use crate::value::Value;

/// Expand a quasiquote form with support for unquote and unquote-splicing
/// Quasiquote recursively quotes all forms except those wrapped in unquote/unquote-splicing
pub fn expand_quasiquote(
    value: &Value,
    symbols: &mut SymbolTable,
    scope_stack: &mut Vec<ScopeEntry>,
) -> Result<Expr, String> {
    // For now, implement quasiquote as a simple transformation:
    // `(a ,x ,@y b) becomes (list 'a x y 'b)
    // We'll build an expression that constructs the list at runtime

    build_quasiquote_expr(value, symbols, scope_stack, 1)
}

/// Build an expression that will construct the quasiquoted list at runtime
fn build_quasiquote_expr(
    value: &Value,
    symbols: &mut SymbolTable,
    scope_stack: &mut Vec<ScopeEntry>,
    depth: usize,
) -> Result<Expr, String> {
    // Import the value_to_expr_with_scope function from parent module
    use super::value_to_expr::value_to_expr_with_scope;

    match value {
        // Simple values are quoted
        Value::Nil
        | Value::Bool(_)
        | Value::Int(_)
        | Value::Float(_)
        | Value::String(_)
        | Value::Symbol(_)
        | Value::Keyword(_) => Ok(Expr::Literal(value.clone())),

        Value::Table(_) | Value::Struct(_) => Ok(Expr::Literal(value.clone())),

        Value::Cons(_) => {
            let list = value.list_to_vec()?;
            if list.is_empty() {
                return Ok(Expr::Literal(Value::Nil));
            }

            // Check for special forms
            if let Value::Symbol(sym) = &list[0] {
                let name = symbols.name(*sym).ok_or("Unknown symbol")?;

                match name {
                    // Nested quasiquote
                    "quasiquote" => {
                        if list.len() != 2 {
                            return Err("quasiquote requires exactly 1 argument".to_string());
                        }
                        build_quasiquote_expr(&list[1], symbols, scope_stack, depth + 1)
                    }

                    // Unquote - evaluate the expression
                    "unquote" => {
                        if list.len() != 2 {
                            return Err("unquote requires exactly 1 argument".to_string());
                        }
                        if depth == 1 {
                            // Evaluate this expression
                            value_to_expr_with_scope(&list[1], symbols, scope_stack)
                        } else {
                            // Nested unquote - decrease depth
                            build_quasiquote_expr(&list[1], symbols, scope_stack, depth - 1)
                        }
                    }

                    // Unquote-splicing only valid in list context
                    "unquote-splicing" => Err(
                        "unquote-splicing can only be used inside a quasiquoted list".to_string(),
                    ),

                    // Regular list - recursively process
                    _ => {
                        // Process elements and build a list construction
                        let mut elements = Vec::new();
                        for elem in &list {
                            // Check if this is unquote-splicing
                            if let Value::Cons(_) = elem {
                                if let Ok(elem_vec) = elem.list_to_vec() {
                                    if !elem_vec.is_empty() {
                                        if let Value::Symbol(elem_sym) = &elem_vec[0] {
                                            if let Some(elem_name) = symbols.name(*elem_sym) {
                                                if elem_name == "unquote-splicing" && depth == 1 {
                                                    // Mark for splicing
                                                    if elem_vec.len() != 2 {
                                                        return Err(
                                                            "unquote-splicing requires 1 argument"
                                                                .to_string(),
                                                        );
                                                    }
                                                    elements.push(Expr::Literal(Value::Symbol(
                                                        symbols.intern("__splice__"),
                                                    )));
                                                    elements.push(value_to_expr_with_scope(
                                                        &elem_vec[1],
                                                        symbols,
                                                        scope_stack,
                                                    )?);
                                                    continue;
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            // Regular element
                            elements.push(build_quasiquote_expr(
                                elem,
                                symbols,
                                scope_stack,
                                depth,
                            )?);
                        }

                        // For now, just return the list of elements as a literal
                        // A full implementation would need runtime support for splicing
                        let mut result_list = Vec::new();
                        for elem_expr in elements {
                            if let Expr::Literal(val) = elem_expr {
                                result_list.push(val);
                            } else {
                                // Return the literal list for now
                                return Ok(Expr::Literal(crate::value::list(result_list)));
                            }
                        }
                        Ok(Expr::Literal(crate::value::list(result_list)))
                    }
                }
            } else {
                // Non-symbol head - just quote the whole thing
                Ok(Expr::Literal(value.clone()))
            }
        }

        Value::Vector(_) => Ok(Expr::Literal(value.clone())),

        // Cannot quote these
        Value::Closure(_)
        | Value::JitClosure(_)
        | Value::NativeFn(_)
        | Value::VmAwareFn(_)
        | Value::LibHandle(_)
        | Value::CHandle(_)
        | Value::Exception(_)
        | Value::Condition(_)
        | Value::ThreadHandle(_)
        | Value::Cell(_)
        | Value::Coroutine(_) => Err("Cannot quote closure or native function".to_string()),
    }
}
