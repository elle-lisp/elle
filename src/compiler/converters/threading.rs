use super::super::ast::Expr;
use crate::symbol::SymbolTable;
use crate::value::{SymbolId, Value};

/// Handle thread-first macro: (-> value form1 form2 ...)
/// Inserts value as the FIRST argument to each form
/// Example: (-> 5 (+ 10) (* 2)) => (* (+ 5 10) 2)
pub fn handle_thread_first(
    list: &[Value],
    symbols: &mut SymbolTable,
    scope_stack: &mut Vec<Vec<SymbolId>>,
) -> Result<Expr, String> {
    use super::value_to_expr::value_to_expr_with_scope;

    if list.len() < 2 {
        return Err("-> requires at least a value and one form".to_string());
    }

    // Start with the value
    let mut result = value_to_expr_with_scope(&list[1], symbols, scope_stack)?;

    // Thread through each form
    for form in &list[2..] {
        result = if let Ok(form_vec) = form.list_to_vec() {
            if form_vec.is_empty() {
                return Err("Cannot thread through empty form".to_string());
            }
            // Insert result as the first argument
            let func = value_to_expr_with_scope(&form_vec[0], symbols, scope_stack)?;
            let mut args = vec![result];
            for arg in &form_vec[1..] {
                args.push(value_to_expr_with_scope(arg, symbols, scope_stack)?);
            }
            Expr::Call {
                func: Box::new(func),
                args,
                tail: false,
            }
        } else {
            // If form is not a list, treat it as a function call with result as arg
            Expr::Call {
                func: Box::new(value_to_expr_with_scope(form, symbols, scope_stack)?),
                args: vec![result],
                tail: false,
            }
        };
    }

    Ok(result)
}

/// Handle thread-last macro: (->> value form1 form2 ...)
/// Inserts value as the LAST argument to each form
/// Example: (->> 5 (+ 10) (* 2)) => (* 2 (+ 10 5))
pub fn handle_thread_last(
    list: &[Value],
    symbols: &mut SymbolTable,
    scope_stack: &mut Vec<Vec<SymbolId>>,
) -> Result<Expr, String> {
    use super::value_to_expr::value_to_expr_with_scope;

    if list.len() < 2 {
        return Err("->> requires at least a value and one form".to_string());
    }

    // Start with the value
    let mut result = value_to_expr_with_scope(&list[1], symbols, scope_stack)?;

    // Thread through each form
    for form in &list[2..] {
        result = if let Ok(form_vec) = form.list_to_vec() {
            if form_vec.is_empty() {
                return Err("Cannot thread through empty form".to_string());
            }
            // Collect arguments up to the end, then add result as last
            let func = value_to_expr_with_scope(&form_vec[0], symbols, scope_stack)?;
            let mut args = Vec::new();
            for arg in &form_vec[1..] {
                args.push(value_to_expr_with_scope(arg, symbols, scope_stack)?);
            }
            args.push(result);
            Expr::Call {
                func: Box::new(func),
                args,
                tail: false,
            }
        } else {
            // If form is not a list, treat it as a function call with result as arg
            Expr::Call {
                func: Box::new(value_to_expr_with_scope(form, symbols, scope_stack)?),
                args: vec![result],
                tail: false,
            }
        };
    }

    Ok(result)
}
