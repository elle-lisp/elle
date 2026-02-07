use super::analysis::{analyze_capture_usage, analyze_free_vars};
use super::ast::{Expr, Pattern};
use super::macros::expand_macro;
use super::patterns::value_to_pattern;
use crate::symbol::SymbolTable;
use crate::value::{SymbolId, Value};
use std::collections::HashSet;

/// Extract all variable bindings from a pattern
fn extract_pattern_variables(pattern: &Pattern) -> Vec<SymbolId> {
    let mut vars = Vec::new();
    match pattern {
        Pattern::Var(sym_id) => {
            vars.push(*sym_id);
        }
        Pattern::Wildcard | Pattern::Literal(_) | Pattern::Nil => {
            // These don't bind variables
        }
        Pattern::List(patterns) => {
            // Extract variables from all list elements
            for p in patterns {
                vars.extend(extract_pattern_variables(p));
            }
        }
        Pattern::Cons { head, tail } => {
            // Extract from head and tail
            vars.extend(extract_pattern_variables(head));
            vars.extend(extract_pattern_variables(tail));
        }
        Pattern::Guard { pattern: inner, .. } => {
            // Extract from inner pattern
            vars.extend(extract_pattern_variables(inner));
        }
    }
    vars
}

/// Adjust variable indices in an expression to account for the closure environment layout.
/// The closure environment is laid out as [captures..., parameters...]
///
/// During parsing, Var nodes contain indices relative to the scope_stack at parse time.
/// After computing captures, we need to convert these to absolute indices in the final environment.
///
/// For each Var:
/// - If it's a parameter of the current lambda: map to captures.len() + position_in_params
/// - If it's a captured value: map to position_in_captures_list
fn adjust_var_indices(expr: &mut Expr, captures: &[(SymbolId, usize, usize)], params: &[SymbolId]) {
    // Build a map of captures to their position in the list
    let mut capture_map: std::collections::HashMap<SymbolId, usize> =
        std::collections::HashMap::new();
    for (i, (sym, _, _)) in captures.iter().enumerate() {
        capture_map.insert(*sym, i);
    }

    // Build a map of parameters to their position in the list
    let mut param_map: std::collections::HashMap<SymbolId, usize> =
        std::collections::HashMap::new();
    for (i, sym) in params.iter().enumerate() {
        param_map.insert(*sym, i);
    }

    match expr {
        Expr::Var(sym_id, _depth, index) => {
            // Adjust indices for variables that are captures or parameters of this lambda
            // Both depth==0 (current scope params) and depth>0 (captured vars) need adjustment
            // because the index is currently relative to scope_stack at parse time, not the
            // final closure environment [captures..., parameters...]
            if let Some(cap_pos) = capture_map.get(sym_id) {
                // This variable is a capture - map to its position in the captures list
                *index = *cap_pos;
            } else if let Some(param_pos) = param_map.get(sym_id) {
                // This variable is a parameter - map to captures.len() + position
                *index = captures.len() + param_pos;
            }
            // Otherwise, it's a global or something from an even outer scope
        }
        Expr::If { cond, then, else_ } => {
            adjust_var_indices(cond, captures, params);
            adjust_var_indices(then, captures, params);
            adjust_var_indices(else_, captures, params);
        }
        Expr::Cond { clauses, else_body } => {
            for (test, body) in clauses {
                adjust_var_indices(test, captures, params);
                adjust_var_indices(body, captures, params);
            }
            if let Some(else_expr) = else_body {
                adjust_var_indices(else_expr, captures, params);
            }
        }
        Expr::Begin(exprs) => {
            for e in exprs {
                adjust_var_indices(e, captures, params);
            }
        }
        Expr::Block(exprs) => {
            for e in exprs {
                adjust_var_indices(e, captures, params);
            }
        }
        Expr::Call { func, args, .. } => {
            adjust_var_indices(func, captures, params);
            for arg in args {
                adjust_var_indices(arg, captures, params);
            }
        }
        Expr::Lambda { .. } => {
            // Don't adjust nested lambda bodies at all - they have already been fully processed
            // with their own capture scopes, parameters, and indices when they were parsed.
            // Recursing into them would incorrectly adjust already-correct indices.
        }
        Expr::Let { bindings, body } => {
            for (_, expr) in bindings {
                adjust_var_indices(expr, captures, params);
            }
            adjust_var_indices(body, captures, params);
        }
        Expr::Letrec { bindings, body } => {
            for (_, expr) in bindings {
                adjust_var_indices(expr, captures, params);
            }
            adjust_var_indices(body, captures, params);
        }
        Expr::Set { value, .. } => {
            adjust_var_indices(value, captures, params);
        }
        Expr::While { cond, body } => {
            adjust_var_indices(cond, captures, params);
            adjust_var_indices(body, captures, params);
        }
        Expr::For { iter, body, .. } => {
            adjust_var_indices(iter, captures, params);
            adjust_var_indices(body, captures, params);
        }
        Expr::Match {
            value,
            patterns,
            default,
        } => {
            adjust_var_indices(value, captures, params);
            for (_, expr) in patterns {
                adjust_var_indices(expr, captures, params);
            }
            if let Some(default_expr) = default {
                adjust_var_indices(default_expr, captures, params);
            }
        }
        Expr::Try {
            body,
            catch,
            finally,
        } => {
            adjust_var_indices(body, captures, params);
            if let Some((_, handler)) = catch {
                adjust_var_indices(handler, captures, params);
            }
            if let Some(finally_expr) = finally {
                adjust_var_indices(finally_expr, captures, params);
            }
        }
        Expr::And(exprs) | Expr::Or(exprs) => {
            for e in exprs {
                adjust_var_indices(e, captures, params);
            }
        }
        Expr::DefMacro { body, .. } => {
            adjust_var_indices(body, captures, params);
        }
        // Literals, GlobalVar, etc. don't need adjustment
        _ => {}
    }
}

/// Expand a quasiquote form with support for unquote and unquote-splicing
/// Quasiquote recursively quotes all forms except those wrapped in unquote/unquote-splicing
fn expand_quasiquote(
    value: &Value,
    symbols: &mut SymbolTable,
    scope_stack: &mut Vec<Vec<SymbolId>>,
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
    scope_stack: &mut Vec<Vec<SymbolId>>,
    depth: usize,
) -> Result<Expr, String> {
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
        | Value::NativeFn(_)
        | Value::LibHandle(_)
        | Value::CHandle(_)
        | Value::Exception(_) => Err("Cannot quote closure or native function".to_string()),
    }
}

/// Simple value-to-expr conversion for bootstrap
/// This is a simple tree-walking approach before full macro expansion
pub fn value_to_expr(value: &Value, symbols: &mut SymbolTable) -> Result<Expr, String> {
    let mut expr = value_to_expr_with_scope(value, symbols, &mut Vec::new())?;
    super::capture_resolution::resolve_captures(&mut expr);
    mark_tail_calls(&mut expr, true);
    Ok(expr)
}

/// Convert a value to an expression, tracking local variable scopes
/// The scope_stack contains local bindings (as Vec for ordering) at each nesting level
fn value_to_expr_with_scope(
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
                        let exprs: Result<Vec<_>, _> = list[1..]
                            .iter()
                            .map(|v| value_to_expr_with_scope(v, symbols, scope_stack))
                            .collect();
                        Ok(Expr::Begin(exprs?))
                    }

                    "block" => {
                        let exprs: Result<Vec<_>, _> = list[1..]
                            .iter()
                            .map(|v| value_to_expr_with_scope(v, symbols, scope_stack))
                            .collect();
                        Ok(Expr::Block(exprs?))
                    }

                    "lambda" => {
                        if list.len() < 3 {
                            return Err("lambda requires at least 2 arguments".to_string());
                        }

                        let params = list[1].list_to_vec()?;
                        let param_syms: Result<Vec<_>, _> =
                            params.iter().map(|p| p.as_symbol()).collect();
                        let param_syms = param_syms?;

                        // Push a new scope with the lambda parameters (as Vec for ordered indices)
                        scope_stack.push(param_syms.clone());

                        let body_exprs: Result<Vec<_>, _> = list[2..]
                            .iter()
                            .map(|v| value_to_expr_with_scope(v, symbols, scope_stack))
                            .collect();

                        // Pop the lambda's scope
                        scope_stack.pop();

                        let body_exprs = body_exprs?;
                        let body = if body_exprs.len() == 1 {
                            Box::new(body_exprs[0].clone())
                        } else {
                            Box::new(Expr::Begin(body_exprs))
                        };

                        // Analyze free variables that need to be captured
                        let mut local_bindings = std::collections::HashSet::new();
                        for param in &param_syms {
                            local_bindings.insert(*param);
                        }
                        let free_vars = analyze_free_vars(&body, &local_bindings);

                        // Convert free vars to captures, resolving their scope location
                        // We need to distinguish: is this a global, or from an outer scope?
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
                        let candidates: HashSet<SymbolId> =
                            captures.iter().map(|(sym, _, _)| *sym).collect();
                        let actually_used =
                            analyze_capture_usage(&body, &local_bindings, &candidates);
                        let captures: Vec<_> = captures
                            .into_iter()
                            .filter(|(sym, _, _)| actually_used.contains(sym))
                            .collect();

                        // Adjust variable indices in body to account for closure environment layout
                        // The closure environment is [captures..., parameters...]
                        let mut adjusted_body = body;
                        adjust_var_indices(&mut adjusted_body, &captures, &param_syms);

                        Ok(Expr::Lambda {
                            params: param_syms,
                            body: adjusted_body,
                            captures,
                        })
                    }

                    "define" => {
                        if list.len() != 3 {
                            return Err("define requires exactly 2 arguments".to_string());
                        }
                        let name = list[1].as_symbol()?;
                        let value =
                            Box::new(value_to_expr_with_scope(&list[2], symbols, scope_stack)?);
                        Ok(Expr::Define { name, value })
                    }

                    "let" => {
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
                                return Err(
                                    "Each let binding must be a [var expr] pair".to_string()
                                );
                            }
                            let var = binding_list[0].as_symbol()?;
                            param_syms.push(var);

                            // Parse the binding expression in the current scope
                            // (bindings cannot reference previous bindings or let-bound variables)
                            let expr =
                                value_to_expr_with_scope(&binding_list[1], symbols, scope_stack)?;
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
                        let mut local_bindings = std::collections::HashSet::new();
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
                        let candidates: HashSet<SymbolId> =
                            captures.iter().map(|(sym, _, _)| *sym).collect();
                        let actually_used =
                            analyze_capture_usage(&body, &local_bindings, &candidates);
                        let captures: Vec<_> = captures
                            .into_iter()
                            .filter(|(sym, _, _)| actually_used.contains(sym))
                            .collect();

                        // Adjust variable indices in body to account for closure environment layout
                        let mut adjusted_body = body;
                        adjust_var_indices(&mut adjusted_body, &captures, &param_syms);

                        // Create lambda: (lambda (var1 var2 ...) body...)
                        let lambda = Expr::Lambda {
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

                    "let*" => {
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
                                return Err(
                                    "Each let* binding must be a [var expr] pair".to_string()
                                );
                            }
                            let var = binding_list[0].as_symbol()?;
                            param_syms.push(var);

                            // Parse binding expression WITH PREVIOUS BINDINGS IN SCOPE
                            // This allows y = (+ x 1) where x was previously bound
                            let expr =
                                value_to_expr_with_scope(&binding_list[1], symbols, scope_stack)?;
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
                        let mut local_bindings = std::collections::HashSet::new();
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
                        let candidates: HashSet<SymbolId> =
                            captures.iter().map(|(sym, _, _)| *sym).collect();
                        let actually_used =
                            analyze_capture_usage(&body, &local_bindings, &candidates);
                        let captures: Vec<_> = captures
                            .into_iter()
                            .filter(|(sym, _, _)| actually_used.contains(sym))
                            .collect();

                        // Adjust variable indices in body to account for closure environment layout
                        let mut adjusted_body = body;
                        adjust_var_indices(&mut adjusted_body, &captures, &param_syms);

                        // Create lambda: (lambda (var1 var2 ...) body...)
                        let lambda = Expr::Lambda {
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

                    "letrec" => {
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
                                return Err(
                                    "Each letrec binding must be a [var expr] pair".to_string()
                                );
                            }
                            param_syms.push(binding_list[0].as_symbol()?);
                        }

                        // Second pass: parse binding expressions
                        // Names are NOT in scope during binding expression parsing
                        // They'll reference each other as GlobalVar, resolved at runtime via scope_stack
                        for binding in &bindings_vec {
                            let binding_list = binding.list_to_vec()?;
                            let expr =
                                value_to_expr_with_scope(&binding_list[1], symbols, scope_stack)?;
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

                    "try" => {
                        // Syntax: (try <body> (catch <var> <handler>) (finally <expr>)?)
                        if list.len() < 2 {
                            return Err("try requires at least a body".to_string());
                        }

                        let body =
                            Box::new(value_to_expr_with_scope(&list[1], symbols, scope_stack)?);
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
                                                return Err("catch requires exactly 2 arguments (variable and handler)".to_string());
                                            }
                                            let var = v[1].as_symbol()?;
                                            let handler = Box::new(value_to_expr_with_scope(
                                                &v[2],
                                                symbols,
                                                scope_stack,
                                            )?);
                                            catch_clause = Some((var, handler));
                                        }
                                        "finally" => {
                                            if v.len() != 2 {
                                                return Err("finally requires exactly 1 argument"
                                                    .to_string());
                                            }
                                            finally_clause =
                                                Some(Box::new(value_to_expr_with_scope(
                                                    &v[1],
                                                    symbols,
                                                    scope_stack,
                                                )?));
                                        }
                                        _ => {
                                            return Err(format!(
                                                "Unknown clause in try: {}",
                                                keyword_str
                                            ));
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

                    "match" => {
                        // Syntax: (match value (pattern1 result1) (pattern2 result2) ... [default])
                        if list.len() < 2 {
                            return Err("match requires at least a value".to_string());
                        }

                        let value =
                            Box::new(value_to_expr_with_scope(&list[1], symbols, scope_stack)?);
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
                                    let pattern = value_to_pattern(&clause_vec[0], symbols)?;

                                    // Extract pattern variables
                                    let pattern_vars = extract_pattern_variables(&pattern);

                                    // If there are pattern variables, wrap the body in a lambda
                                    // that binds them to the matched value
                                    let result = if !pattern_vars.is_empty() {
                                        // Add pattern variables to scope for parsing the body
                                        let mut new_scope_stack = scope_stack.clone();
                                        new_scope_stack.push(pattern_vars.clone());

                                        // Parse the result in the new scope
                                        let body_expr = value_to_expr_with_scope(
                                            &clause_vec[1],
                                            symbols,
                                            &mut new_scope_stack,
                                        )?;

                                        // Transform: (lambda (var1 var2 ...) body_expr)
                                        // This binds the pattern variables for use in the body
                                        Expr::Lambda {
                                            params: pattern_vars,
                                            body: Box::new(body_expr),
                                            captures: Vec::new(),
                                        }
                                    } else {
                                        // No variables to bind
                                        value_to_expr_with_scope(
                                            &clause_vec[1],
                                            symbols,
                                            scope_stack,
                                        )?
                                    };

                                    patterns.push((pattern, result));
                                } else {
                                    return Err(
                                        "Pattern clause must have pattern and result".to_string()
                                    );
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
                        let body_str = format!("{}", list[3]);

                        // Register the macro in the symbol table
                        use crate::symbol::MacroDef;
                        symbols.define_macro(MacroDef {
                            name,
                            params: params.clone(),
                            body: body_str,
                        });

                        let body =
                            Box::new(value_to_expr_with_scope(&list[3], symbols, scope_stack)?);

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

                    "->" => {
                        // Syntax: (-> value form1 form2 ...)
                        // Thread first: inserts value as the FIRST argument to each form
                        // Example: (-> 5 (+ 10) (* 2)) => (* (+ 5 10) 2)
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
                                let func =
                                    value_to_expr_with_scope(&form_vec[0], symbols, scope_stack)?;
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
                                    func: Box::new(value_to_expr_with_scope(
                                        form,
                                        symbols,
                                        scope_stack,
                                    )?),
                                    args: vec![result],
                                    tail: false,
                                }
                            };
                        }

                        Ok(result)
                    }

                    "->>" => {
                        // Syntax: (->> value form1 form2 ...)
                        // Thread last: inserts value as the LAST argument to each form
                        // Example: (->> 5 (+ 10) (* 2)) => (* 2 (+ 10 5))
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
                                let func =
                                    value_to_expr_with_scope(&form_vec[0], symbols, scope_stack)?;
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
                                    func: Box::new(value_to_expr_with_scope(
                                        form,
                                        symbols,
                                        scope_stack,
                                    )?),
                                    args: vec![result],
                                    tail: false,
                                }
                            };
                        }

                        Ok(result)
                    }

                    "cond" => {
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
                                            return Err(
                                                "cond can have at most one else clause".to_string()
                                            );
                                        }
                                        // The else clause body can be multiple expressions
                                        let body_exprs: Result<Vec<_>, _> = clause_vec[1..]
                                            .iter()
                                            .map(|v| {
                                                value_to_expr_with_scope(v, symbols, scope_stack)
                                            })
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
                                return Err(
                                    "cond clause must have at least a test and a body".to_string()
                                );
                            }

                            let test =
                                value_to_expr_with_scope(&clause_vec[0], symbols, scope_stack)?;

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
fn mark_tail_calls(expr: &mut Expr, in_tail: bool) {
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
