use super::analysis::analyze_free_vars;
use super::ast::Expr;
use super::macros::expand_macro;
use super::patterns::value_to_pattern;
use crate::symbol::SymbolTable;
use crate::value::Value;

/// Simple value-to-expr conversion for bootstrap
/// This is a simple tree-walking approach before full macro expansion
pub fn value_to_expr(value: &Value, symbols: &mut SymbolTable) -> Result<Expr, String> {
    match value {
        Value::Nil | Value::Bool(_) | Value::Int(_) | Value::Float(_) | Value::String(_) => {
            Ok(Expr::Literal(value.clone()))
        }

        Value::Symbol(id) => {
            // Treat all symbols as global vars for now
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
                    "quote" => {
                        if list.len() != 2 {
                            return Err("quote requires exactly 1 argument".to_string());
                        }
                        Ok(Expr::Literal(list[1].clone()))
                    }

                    "if" => {
                        if list.len() < 3 || list.len() > 4 {
                            return Err("if requires 2 or 3 arguments".to_string());
                        }
                        let cond = Box::new(value_to_expr(&list[1], symbols)?);
                        let then = Box::new(value_to_expr(&list[2], symbols)?);
                        let else_ = if list.len() == 4 {
                            Box::new(value_to_expr(&list[3], symbols)?)
                        } else {
                            Box::new(Expr::Literal(Value::Nil))
                        };
                        Ok(Expr::If { cond, then, else_ })
                    }

                    "begin" => {
                        let exprs: Result<Vec<_>, _> = list[1..]
                            .iter()
                            .map(|v| value_to_expr(v, symbols))
                            .collect();
                        Ok(Expr::Begin(exprs?))
                    }

                    "lambda" => {
                        if list.len() < 3 {
                            return Err("lambda requires at least 2 arguments".to_string());
                        }

                        let params = list[1].list_to_vec()?;
                        let param_syms: Result<Vec<_>, _> =
                            params.iter().map(|p| p.as_symbol()).collect();
                        let param_syms = param_syms?;

                        let body_exprs: Result<Vec<_>, _> = list[2..]
                            .iter()
                            .map(|v| value_to_expr(v, symbols))
                            .collect();
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

                        // Convert free vars to captures (with placeholder depth/index)
                        // These will be resolved at runtime
                        let captures: Vec<_> = free_vars
                            .iter()
                            .map(|sym| (*sym, 0, 0)) // Depth and index will be resolved later
                            .collect();

                        Ok(Expr::Lambda {
                            params: param_syms,
                            body,
                            captures,
                        })
                    }

                    "define" => {
                        if list.len() != 3 {
                            return Err("define requires exactly 2 arguments".to_string());
                        }
                        let name = list[1].as_symbol()?;
                        let value = Box::new(value_to_expr(&list[2], symbols)?);
                        Ok(Expr::Define { name, value })
                    }

                    "set!" => {
                        if list.len() != 3 {
                            return Err("set! requires exactly 2 arguments".to_string());
                        }
                        let var = list[1].as_symbol()?;
                        let value = Box::new(value_to_expr(&list[2], symbols)?);
                        Ok(Expr::Set {
                            var,
                            depth: 0,
                            index: 0,
                            value,
                        })
                    }

                    "try" => {
                        // Syntax: (try <body> (catch <var> <handler>) (finally <expr>)?)
                        if list.len() < 2 {
                            return Err("try requires at least a body".to_string());
                        }

                        let body = Box::new(value_to_expr(&list[1], symbols)?);
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
                                            let handler = Box::new(value_to_expr(&v[2], symbols)?);
                                            catch_clause = Some((var, handler));
                                        }
                                        "finally" => {
                                            if v.len() != 2 {
                                                return Err("finally requires exactly 1 argument"
                                                    .to_string());
                                            }
                                            finally_clause =
                                                Some(Box::new(value_to_expr(&v[1], symbols)?));
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

                        let value = Box::new(value_to_expr(&list[1], symbols)?);
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
                                    default =
                                        Some(Box::new(value_to_expr(&clause_vec[0], symbols)?));
                                } else if clause_vec.len() == 2 {
                                    // Pattern and result
                                    let pattern = value_to_pattern(&clause_vec[0], symbols)?;
                                    let result = value_to_expr(&clause_vec[1], symbols)?;
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
                        let args = vec![value_to_expr(&list[1], symbols)?];
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

                        let body = Box::new(value_to_expr(&list[3], symbols)?);

                        Ok(Expr::DefMacro { name, params, body })
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
                                    return value_to_expr(&expanded, symbols);
                                }
                            }
                        }

                        // Regular function call
                        let func = Box::new(value_to_expr(first, symbols)?);
                        let args: Result<Vec<_>, _> = list[1..]
                            .iter()
                            .map(|v| value_to_expr(v, symbols))
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
                let func = Box::new(value_to_expr(first, symbols)?);
                let args: Result<Vec<_>, _> = list[1..]
                    .iter()
                    .map(|v| value_to_expr(v, symbols))
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
