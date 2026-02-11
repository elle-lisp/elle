use super::super::analysis::{analyze_capture_usage, analyze_free_vars};
use super::super::ast::Expr;
use super::variable_analysis::{adjust_var_indices, pre_register_defines};
use crate::symbol::SymbolTable;
use crate::value::{SymbolId, Value};
use std::collections::HashSet;

/// Helper function to convert lambda expressions
/// Extracted to reduce stack frame size of value_to_expr_with_scope
#[inline(never)]
pub fn convert_lambda(
    list: &[Value],
    symbols: &mut SymbolTable,
    scope_stack: &mut Vec<Vec<SymbolId>>,
) -> Result<Expr, String> {
    use super::value_to_expr::value_to_expr_with_scope;

    if list.len() < 3 {
        return Err("fn requires at least 2 arguments".to_string());
    }

    let params = list[1].list_to_vec()?;
    let param_syms: Result<Vec<_>, _> = params.iter().map(|p| p.as_symbol()).collect();
    let param_syms = param_syms?;

    // Push a new scope with the lambda parameters (as Vec for ordered indices)
    scope_stack.push(param_syms.clone());

    // Pre-scan body for define names to enable mutual recursion and self-recursion
    // All locally-defined names must be in scope before any lambda values are parsed
    pre_register_defines(&list[2..], symbols, scope_stack);

    // Process body expressions sequentially to handle variable definitions properly
    let mut body_exprs_vec = Vec::new();
    for expr_val in &list[2..] {
        let expr = value_to_expr_with_scope(expr_val, symbols, scope_stack)?;
        body_exprs_vec.push(expr);
    }

    let body_exprs = body_exprs_vec;

    let body = if body_exprs.len() == 1 {
        Box::new(body_exprs[0].clone())
    } else {
        Box::new(Expr::Begin(body_exprs))
    };

    // Get the current lambda's scope (which includes all locally-defined variables)
    let lambda_scope = scope_stack.last().unwrap().clone();

    // Identify which variables in the lambda scope are defined in the lambda body
    // (as opposed to being parameters)
    let mut locally_defined_vars: Vec<SymbolId> = lambda_scope
        .iter()
        .filter(|var| !param_syms.contains(var))
        .copied()
        .collect();
    // Sort for deterministic ordering
    locally_defined_vars.sort();

    // Analyze free variables that need to be captured
    // IMPORTANT: Do this BEFORE popping scope_stack, so locally-defined
    // variables from the lambda body are still visible
    let mut local_bindings = HashSet::new();
    for param in &param_syms {
        local_bindings.insert(*param);
    }
    // Also include locally-defined variables so they're not treated as free variables
    // They will be tracked separately in the `locals` field of the Lambda expression
    for local_var in &locally_defined_vars {
        local_bindings.insert(*local_var);
    }
    let free_vars = analyze_free_vars(&body, &local_bindings);

    // Identify locally-defined variables that are captured by nested lambdas
    // These need special handling (cell wrapping) for Phase 4
    let mut sorted_free_vars: Vec<_> = free_vars.iter().copied().collect();
    sorted_free_vars.sort(); // Deterministic ordering

    let captures: Vec<_> = sorted_free_vars
        .iter()
        .map(|sym| {
            // Look up in scope stack to determine if global or local
            for (reverse_idx, scope) in scope_stack.iter().enumerate().rev() {
                if let Some(local_index) = scope.iter().position(|s| s == sym) {
                    let depth = scope_stack.len() - 1 - reverse_idx;
                    return (*sym, depth, local_index);
                }
            }
            // If not found in scope stack, it's a global variable
            (*sym, 0, usize::MAX)
        })
        .collect();

    // Pop the lambda's scope NOW, after we've analyzed captures
    scope_stack.pop();

    // Dead capture elimination: filter out captures that aren't actually used in the body
    let candidates: HashSet<SymbolId> = captures.iter().map(|(sym, _, _)| *sym).collect();
    let actually_used = analyze_capture_usage(&body, &local_bindings, &candidates);
    let captures: Vec<_> = captures
        .into_iter()
        .filter(|(sym, _, _)| actually_used.contains(sym))
        .collect();

    // Adjust variable indices in body to account for closure environment layout
    // The closure environment is [captures..., parameters..., locals...]
    let mut adjusted_body = body;
    adjust_var_indices(
        &mut adjusted_body,
        &captures,
        &param_syms,
        &locally_defined_vars,
    );

    Ok(Expr::Lambda {
        params: param_syms,
        body: adjusted_body,
        captures,
        locals: locally_defined_vars,
    })
}

/// Helper function to convert let expressions
/// Extracted to reduce stack frame size of value_to_expr_with_scope
#[inline(never)]
pub fn convert_let(
    list: &[Value],
    symbols: &mut SymbolTable,
    scope_stack: &mut Vec<Vec<SymbolId>>,
) -> Result<Expr, String> {
    use super::value_to_expr::value_to_expr_with_scope;

    // Syntax: (let ((var1 expr1) (var2 expr2) ...) body...)
    // Transform to: ((lambda (var1 var2 ...) body...) expr1 expr2 ...)
    if list.len() < 2 {
        return Err("let requires at least a binding vector".to_string());
    }

    // Parse the bindings vector
    let bindings_vec = list[1].list_to_vec()?;
    let mut param_syms = Vec::new();
    let mut binding_exprs = Vec::new();

    for binding in bindings_vec {
        let binding_list = binding.list_to_vec()?;
        if binding_list.len() != 2 {
            return Err("Each let binding must be a [var expr] pair".to_string());
        }
        let var = binding_list[0].as_symbol()?;
        param_syms.push(var);

        // Parse the binding expression in the current scope
        // (bindings cannot reference previous bindings or let-bound variables)
        let expr = value_to_expr_with_scope(&binding_list[1], symbols, scope_stack)?;
        binding_exprs.push(expr);
    }

    // Parse the body (one or more expressions)
    // Body can reference let-bound variables, so we add them to scope
    scope_stack.push(param_syms.clone());

    let body_exprs: Result<Vec<_>, _> = list[2..]
        .iter()
        .map(|v| value_to_expr_with_scope(v, symbols, scope_stack))
        .collect();

    scope_stack.pop();

    let body_exprs = body_exprs?;
    let body = if body_exprs.len() == 1 {
        Box::new(body_exprs[0].clone())
    } else if body_exprs.is_empty() {
        Box::new(Expr::Literal(Value::Nil))
    } else {
        Box::new(Expr::Begin(body_exprs))
    };

    // Analyze free variables in the body
    let mut local_bindings = HashSet::new();
    for param in &param_syms {
        local_bindings.insert(*param);
    }
    let free_vars = analyze_free_vars(&body, &local_bindings);

    // Convert free vars to captures, resolving their scope location
    let mut sorted_free_vars: Vec<_> = free_vars.iter().copied().collect();
    sorted_free_vars.sort(); // Deterministic ordering

    let captures: Vec<_> = sorted_free_vars
        .iter()
        .map(|sym| {
            // Look up in scope stack to determine if global or local
            for (reverse_idx, scope) in scope_stack.iter().enumerate().rev() {
                if let Some(local_index) = scope.iter().position(|s| s == sym) {
                    let depth = scope_stack.len() - 1 - reverse_idx;
                    return (*sym, depth, local_index);
                }
            }
            // If not found in scope stack, it's a global variable
            (*sym, 0, usize::MAX)
        })
        .collect();

    // Dead capture elimination: filter out captures that aren't actually used in the body
    let candidates: HashSet<SymbolId> = captures.iter().map(|(sym, _, _)| *sym).collect();
    let actually_used = analyze_capture_usage(&body, &local_bindings, &candidates);
    let captures: Vec<_> = captures
        .into_iter()
        .filter(|(sym, _, _)| actually_used.contains(sym))
        .collect();

    // Adjust variable indices in body to account for closure environment layout
    let mut adjusted_body = body;
    adjust_var_indices(&mut adjusted_body, &captures, &param_syms, &[]);

    // Create lambda: (lambda (var1 var2 ...) body...)
    let lambda = Expr::Lambda {
        params: param_syms,
        body: adjusted_body,
        captures,
        locals: vec![], // Let-converted lambdas have no local defines
    };

    // Create call: (lambda expr1 expr2 ...)
    Ok(Expr::Call {
        func: Box::new(lambda),
        args: binding_exprs,
        tail: false,
    })
}

/// Helper function to convert let* expressions
/// Extracted to reduce stack frame size of value_to_expr_with_scope
#[inline(never)]
pub fn convert_let_star(
    list: &[Value],
    symbols: &mut SymbolTable,
    scope_stack: &mut Vec<Vec<SymbolId>>,
) -> Result<Expr, String> {
    use super::value_to_expr::value_to_expr_with_scope;

    // Syntax: (let* ((var1 expr1) (var2 expr2) ...) body...)
    // Let* differs from let in that each binding can reference previous bindings.
    //
    // Strategy: parse binding expressions sequentially, adding each variable
    // to scope as we go. This naturally handles the sequential evaluation.
    // Then create a single large lambda with all parameters, using the
    // parsed expressions as arguments.
    if list.len() < 2 {
        return Err("let* requires at least a binding vector".to_string());
    }

    let bindings_vec = list[1].list_to_vec()?;

    if bindings_vec.is_empty() {
        // (let* () body...) - just evaluate body
        let body_exprs: Result<Vec<_>, _> = list[2..]
            .iter()
            .map(|v| value_to_expr_with_scope(v, symbols, scope_stack))
            .collect();
        let body_exprs = body_exprs?;
        if body_exprs.is_empty() {
            return Ok(Expr::Literal(Value::Nil));
        } else if body_exprs.len() == 1 {
            return Ok(body_exprs[0].clone());
        } else {
            return Ok(Expr::Begin(body_exprs));
        }
    }

    // Parse bindings sequentially with growing scope
    let mut param_syms = Vec::new();
    let mut binding_exprs = Vec::new();
    scope_stack.push(Vec::new());

    for binding in &bindings_vec {
        let binding_list = binding.list_to_vec()?;
        if binding_list.len() != 2 {
            return Err("Each let* binding must be a [var expr] pair".to_string());
        }
        let var = binding_list[0].as_symbol()?;
        param_syms.push(var);

        // Parse binding expression WITH PREVIOUS BINDINGS IN SCOPE
        // This allows y = (+ x 1) where x was previously bound
        let expr = value_to_expr_with_scope(&binding_list[1], symbols, scope_stack)?;
        binding_exprs.push(expr);

        // Add this variable to scope for next binding
        if let Some(current_scope) = scope_stack.last_mut() {
            current_scope.push(var);
        }
    }

    // Parse body with all let* variables in scope
    let body_exprs: Result<Vec<_>, _> = list[2..]
        .iter()
        .map(|v| value_to_expr_with_scope(v, symbols, scope_stack))
        .collect();

    scope_stack.pop();

    let body_exprs = body_exprs?;
    let body = if body_exprs.len() == 1 {
        Box::new(body_exprs[0].clone())
    } else if body_exprs.is_empty() {
        Box::new(Expr::Literal(Value::Nil))
    } else {
        Box::new(Expr::Begin(body_exprs))
    };

    // Analyze free variables
    let mut local_bindings = HashSet::new();
    for param in &param_syms {
        local_bindings.insert(*param);
    }
    let free_vars = analyze_free_vars(&body, &local_bindings);

    // Convert free vars to captures, resolving their scope location
    let mut sorted_free_vars: Vec<_> = free_vars.iter().copied().collect();
    sorted_free_vars.sort(); // Deterministic ordering

    let captures: Vec<_> = sorted_free_vars
        .iter()
        .map(|sym| {
            // Look up in scope stack to determine if global or local
            for (reverse_idx, scope) in scope_stack.iter().enumerate().rev() {
                if let Some(local_index) = scope.iter().position(|s| s == sym) {
                    let depth = scope_stack.len() - 1 - reverse_idx;
                    return (*sym, depth, local_index);
                }
            }
            // If not found in scope stack, it's a global variable
            (*sym, 0, usize::MAX)
        })
        .collect();

    // Dead capture elimination: filter out captures that aren't actually used in the body
    let candidates: HashSet<SymbolId> = captures.iter().map(|(sym, _, _)| *sym).collect();
    let actually_used = analyze_capture_usage(&body, &local_bindings, &candidates);
    let captures: Vec<_> = captures
        .into_iter()
        .filter(|(sym, _, _)| actually_used.contains(sym))
        .collect();

    // Adjust variable indices in body to account for closure environment layout
    let mut adjusted_body = body;
    adjust_var_indices(&mut adjusted_body, &captures, &param_syms, &[]);

    // Create lambda: (lambda (var1 var2 ...) body...)
    let lambda = Expr::Lambda {
        locals: vec![], // Let*-converted lambdas have no local defines
        params: param_syms,
        body: adjusted_body,
        captures,
    };

    // Create call: (lambda expr1 expr2 ...)
    Ok(Expr::Call {
        func: Box::new(lambda),
        args: binding_exprs,
        tail: false,
    })
}

/// Helper function to convert letrec expressions
/// Extracted to reduce stack frame size of value_to_expr_with_scope
#[inline(never)]
pub fn convert_letrec(
    list: &[Value],
    symbols: &mut SymbolTable,
    scope_stack: &mut Vec<Vec<SymbolId>>,
) -> Result<Expr, String> {
    use super::value_to_expr::value_to_expr_with_scope;

    // Syntax: (letrec ((var1 expr1) (var2 expr2) ...) body...)
    // All bindings are visible to all binding expressions and the body.
    // Unlike let, bindings can reference each other.
    if list.len() < 2 {
        return Err("letrec requires at least a binding vector".to_string());
    }

    let bindings_vec = list[1].list_to_vec()?;
    let mut param_syms = Vec::new();
    let mut binding_exprs = Vec::new();

    // First pass: collect all variable names
    for binding in &bindings_vec {
        let binding_list = binding.list_to_vec()?;
        if binding_list.len() != 2 {
            return Err("Each letrec binding must be a [var expr] pair".to_string());
        }
        param_syms.push(binding_list[0].as_symbol()?);
    }

    // Second pass: parse binding expressions
    // Names are NOT in scope during binding expression parsing
    // They'll reference each other as GlobalVar, resolved at runtime via scope_stack
    for binding in &bindings_vec {
        let binding_list = binding.list_to_vec()?;
        let expr = value_to_expr_with_scope(&binding_list[1], symbols, scope_stack)?;
        binding_exprs.push(expr);
    }

    // Parse body in current scope (names also as GlobalVar)
    let body_exprs: Result<Vec<_>, _> = list[2..]
        .iter()
        .map(|v| value_to_expr_with_scope(v, symbols, scope_stack))
        .collect();

    let body_exprs = body_exprs?;
    let body = if body_exprs.len() == 1 {
        Box::new(body_exprs[0].clone())
    } else if body_exprs.is_empty() {
        Box::new(Expr::Literal(Value::Nil))
    } else {
        Box::new(Expr::Begin(body_exprs))
    };

    let bindings: Vec<_> = param_syms.into_iter().zip(binding_exprs).collect();

    Ok(Expr::Letrec { bindings, body })
}
