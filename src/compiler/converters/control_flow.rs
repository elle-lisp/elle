use super::super::ast::Expr;
use super::variable_analysis::extract_pattern_variables;
use super::{ScopeEntry, ScopeType};
use crate::symbol::SymbolTable;
use crate::value::Value;

/// Helper function to convert match expressions
/// Extracted to reduce stack frame size of value_to_expr_with_scope
#[inline(never)]
pub fn convert_match_expr(
    list: &[Value],
    symbols: &mut SymbolTable,
    scope_stack: &mut Vec<ScopeEntry>,
) -> Result<Expr, String> {
    use super::value_to_expr::value_to_expr_with_scope;

    // Syntax: (match value (pattern1 result1) (pattern2 result2) ... [default])
    if list.len() < 2 {
        return Err("match requires at least a value".to_string());
    }

    let value = Box::new(value_to_expr_with_scope(&list[1], symbols, scope_stack)?);
    let mut patterns = Vec::new();
    let mut default = None;

    // Parse pattern clauses
    for clause in &list[2..] {
        if let Ok(clause_vec) = clause.list_to_vec() {
            if clause_vec.is_empty() {
                return Err("Empty pattern clause".to_string());
            }

            // Check if this is a default clause (symbol, not a list)
            if clause_vec.len() == 1 {
                // Single value - treat as default
                default = Some(Box::new(value_to_expr_with_scope(
                    &clause_vec[0],
                    symbols,
                    scope_stack,
                )?));
            } else if clause_vec.len() == 2 {
                // Pattern and result
                let pattern = super::super::patterns::value_to_pattern(&clause_vec[0], symbols)?;

                // Extract pattern variables
                let pattern_vars = extract_pattern_variables(&pattern);

                // If there are pattern variables, wrap the body in a lambda
                // that binds them to the matched value
                let result = if !pattern_vars.is_empty() {
                    let num_vars = pattern_vars.len();
                    // Add pattern variables to scope for parsing the body
                    let mut new_scope_stack = scope_stack.clone();
                    new_scope_stack.push(ScopeEntry {
                        symbols: pattern_vars.clone(),
                        scope_type: ScopeType::Function,
                    });

                    // Parse the result in the new scope
                    let body_expr =
                        value_to_expr_with_scope(&clause_vec[1], symbols, &mut new_scope_stack)?;

                    // Transform: (lambda (var1 var2 ...) body_expr)
                    // This binds the pattern variables for use in the body
                    Expr::Lambda {
                        params: pattern_vars,
                        body: Box::new(body_expr),
                        captures: Vec::new(),
                        num_locals: num_vars,
                        locals: vec![],
                    }
                } else {
                    // No variables to bind
                    value_to_expr_with_scope(&clause_vec[1], symbols, scope_stack)?
                };

                patterns.push((pattern, result));
            } else {
                return Err("Pattern clause must have pattern and result".to_string());
            }
        } else {
            return Err("Expected pattern clause to be a list".to_string());
        }
    }

    Ok(Expr::Match {
        value,
        patterns,
        default,
    })
}

/// Helper function to convert cond expressions
/// Extracted to reduce stack frame size of value_to_expr_with_scope
#[inline(never)]
pub fn convert_cond(
    list: &[Value],
    symbols: &mut SymbolTable,
    scope_stack: &mut Vec<ScopeEntry>,
) -> Result<Expr, String> {
    use super::value_to_expr::value_to_expr_with_scope;

    // Syntax: (cond (test1 body1) (test2 body2) ... [(else body)])
    // A cond expression evaluates test expressions in order until one is truthy,
    // then evaluates and returns its corresponding body.
    // If no tests are truthy and there's an else clause, evaluate the else body.
    // If no tests are truthy and there's no else clause, return nil.
    if list.len() < 2 {
        return Err("cond requires at least one clause".to_string());
    }

    let mut clauses = Vec::new();
    let mut else_body = None;

    // Parse clauses
    for clause in &list[1..] {
        let clause_vec = clause.list_to_vec()?;
        if clause_vec.is_empty() {
            return Err("cond clause cannot be empty".to_string());
        }

        // Check if this is the else clause (single symbol 'else' followed by body)
        if !clause_vec.is_empty() {
            if let Value::Symbol(test_sym) = &clause_vec[0] {
                if let Some("else") = symbols.name(*test_sym) {
                    // This is the else clause
                    if else_body.is_some() {
                        return Err("cond can have at most one else clause".to_string());
                    }
                    // The else clause body can be multiple expressions
                    let body_exprs: Result<Vec<_>, _> = clause_vec[1..]
                        .iter()
                        .map(|v| value_to_expr_with_scope(v, symbols, scope_stack))
                        .collect();
                    let body_exprs = body_exprs?;
                    let body = if body_exprs.is_empty() {
                        Expr::Literal(Value::Nil)
                    } else if body_exprs.len() == 1 {
                        body_exprs[0].clone()
                    } else {
                        Expr::Begin(body_exprs)
                    };
                    else_body = Some(Box::new(body));
                    continue;
                }
            }
        }

        // Regular clause: (test body...)
        if clause_vec.len() < 2 {
            return Err("cond clause must have at least a test and a body".to_string());
        }

        let test = value_to_expr_with_scope(&clause_vec[0], symbols, scope_stack)?;

        // The body can be multiple expressions
        let body_exprs: Result<Vec<_>, _> = clause_vec[1..]
            .iter()
            .map(|v| value_to_expr_with_scope(v, symbols, scope_stack))
            .collect();
        let body_exprs = body_exprs?;
        let body = if body_exprs.is_empty() {
            Expr::Literal(Value::Nil)
        } else if body_exprs.len() == 1 {
            body_exprs[0].clone()
        } else {
            Expr::Begin(body_exprs)
        };

        clauses.push((test, body));
    }

    Ok(Expr::Cond { clauses, else_body })
}
