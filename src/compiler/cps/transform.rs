//! CPS transformation for yielding expressions
//!
//! This module transforms expressions that may yield into CPS form.
//! Pure expressions are preserved as-is, while yielding expressions
//! are converted to continuation-passing style.

use super::{Continuation, CpsExpr};
use crate::binding::VarRef;
use crate::compiler::ast::{CaptureInfo, Expr};
use crate::compiler::effects::{Effect, EffectContext};
use crate::value::{SymbolId, Value};
use std::collections::HashMap;
use std::rc::Rc;

/// CPS transformer
pub struct CpsTransformer<'a> {
    /// Effect context for determining which expressions yield
    effect_ctx: &'a EffectContext,
    /// Next local variable index (for index-based locals)
    next_local_index: usize,
    /// Map from SymbolId to CPS index for current scope
    local_indices: HashMap<SymbolId, usize>,
}

impl<'a> CpsTransformer<'a> {
    /// Create a new CPS transformer
    pub fn new(effect_ctx: &'a EffectContext) -> Self {
        Self {
            effect_ctx,
            next_local_index: 0,
            local_indices: HashMap::new(),
        }
    }

    /// Create a new CPS transformer with initial local index
    /// Used when transforming lambda bodies where captures and params are already allocated
    pub fn with_initial_index(effect_ctx: &'a EffectContext, initial_index: usize) -> Self {
        Self {
            effect_ctx,
            next_local_index: initial_index,
            local_indices: HashMap::new(),
        }
    }

    /// Transform an expression to CPS form
    ///
    /// If the expression is pure, it's wrapped in CpsExpr::Pure.
    /// If it may yield, it's fully CPS-transformed.
    pub fn transform(&mut self, expr: &Expr, cont: Rc<Continuation>) -> CpsExpr {
        // Always check for specific expression types that need transformation
        // even if they're pure (like function calls)
        match expr {
            Expr::Call { .. }
            | Expr::Yield(_)
            | Expr::Let { .. }
            | Expr::Define { .. }
            | Expr::Begin(_)
            | Expr::If { .. }
            | Expr::While { .. }
            | Expr::Lambda { .. }
            | Expr::Var(_)
            | Expr::Literal(_) => {
                // These expressions always go through transform_yielding
                // which handles both pure and yielding cases
                // Note: Var and Literal are included to ensure
                // they get proper CPS representation instead of being wrapped in Pure
                self.transform_yielding(expr, cont)
            }
            _ => {
                // Other expressions - check effect
                let effect = self.effect_ctx.infer(expr);
                if effect.is_pure() {
                    self.transform_pure(expr, cont)
                } else {
                    self.transform_yielding(expr, cont)
                }
            }
        }
    }

    /// Transform a pure expression (wrap it, don't transform)
    fn transform_pure(&mut self, expr: &Expr, cont: Rc<Continuation>) -> CpsExpr {
        // For pure expressions, we just wrap them
        // The continuation will be applied after evaluation
        CpsExpr::Pure {
            expr: expr.clone(),
            continuation: cont,
        }
    }

    /// Transform a yielding expression to CPS
    fn transform_yielding(&mut self, expr: &Expr, cont: Rc<Continuation>) -> CpsExpr {
        match expr {
            // D1: Yield expression
            Expr::Yield(value_expr) => self.transform_yield(value_expr, cont),

            // D2: Function call
            Expr::Call {
                func,
                args,
                tail: _,
            } => self.transform_call(func, args, cont),

            // Literals are always pure
            Expr::Literal(v) => CpsExpr::Literal(v.clone()),

            // Variables - check if CPS-local (from let/for)
            Expr::Var(var_ref) => {
                match var_ref {
                    VarRef::Local { index } | VarRef::Upvalue { index, .. } => {
                        // CPS locals are indexed directly
                        CpsExpr::Var {
                            sym: SymbolId(0), // Symbol not needed for indexed access
                            depth: 0,
                            index: *index,
                        }
                    }
                    VarRef::LetBound { sym } => {
                        // Check if this let-bound variable is a CPS local (from let/for)
                        if let Some(&index) = self.local_indices.get(sym) {
                            CpsExpr::Var {
                                sym: *sym,
                                depth: 0,
                                index,
                            }
                        } else {
                            // Not a CPS local - treat as global
                            CpsExpr::GlobalVar(*sym)
                        }
                    }
                    VarRef::Global { sym } => CpsExpr::GlobalVar(*sym),
                }
            }

            // D4: Let binding
            Expr::Let { bindings, body } => self.transform_let(bindings, body, cont),

            // D4: Begin/sequence
            Expr::Begin(exprs) => self.transform_sequence(exprs, cont),

            // D4: Block expression
            Expr::Block(exprs) => self.transform_block(exprs, cont),

            // D4: If expression
            Expr::If { cond, then, else_ } => self.transform_if(cond, then, else_, cont),

            // D5: While loop
            Expr::While { cond, body } => self.transform_while(cond, body, cont),

            // D5: For loop
            Expr::For { var, iter, body } => self.transform_for(*var, iter, body, cont),

            // D4: And expression
            Expr::And(exprs) => self.transform_and(exprs, cont),

            // D4: Or expression
            Expr::Or(exprs) => self.transform_or(exprs, cont),

            // D4: Cond expression
            Expr::Cond { clauses, else_body } => {
                self.transform_cond(clauses, else_body.as_deref(), cont)
            }

            // Lambda - transform body if it may yield
            Expr::Lambda {
                params,
                body,
                captures,
                num_locals,
                ..
            } => self.transform_lambda(params, body, captures, *num_locals, cont),

            // Internal define - treat like a let binding
            Expr::Define { name, value } => self.transform_define(*name, value, cont),

            // Other expressions - treat as pure for now
            _ => CpsExpr::Pure {
                expr: expr.clone(),
                continuation: cont,
            },
        }
    }

    /// D1: Transform yield expression
    fn transform_yield(&mut self, value_expr: &Expr, cont: Rc<Continuation>) -> CpsExpr {
        // (yield e) becomes a Yield CPS expression
        // The value is evaluated, then yielded
        // The continuation is captured for resumption
        let value_cps = self.transform(value_expr, Continuation::done());
        CpsExpr::Yield {
            value: Box::new(value_cps),
            continuation: cont,
        }
    }

    /// D2: Transform function call
    fn transform_call(&mut self, func: &Expr, args: &[Expr], cont: Rc<Continuation>) -> CpsExpr {
        // Check if the function may yield
        let func_effect = self.infer_call_effect(func, args);

        // Transform function and arguments
        let func_cps = self.transform(func, Continuation::done());
        let args_cps: Vec<CpsExpr> = args
            .iter()
            .map(|a| self.transform(a, Continuation::done()))
            .collect();

        if func_effect.may_yield() {
            // D2: Yielding function - use CPS call
            CpsExpr::CpsCall {
                func: Box::new(func_cps),
                args: args_cps,
                continuation: cont,
            }
        } else {
            // D3: Pure function - preserve native call
            CpsExpr::PureCall {
                func: Box::new(func_cps),
                args: args_cps,
                continuation: cont,
            }
        }
    }

    /// Infer the effect of calling a function
    fn infer_call_effect(&self, func: &Expr, args: &[Expr]) -> Effect {
        self.effect_ctx.infer_call_effect(func, args)
    }

    /// D4: Transform let binding
    fn transform_let(
        &mut self,
        bindings: &[(SymbolId, Expr)],
        body: &Expr,
        cont: Rc<Continuation>,
    ) -> CpsExpr {
        if bindings.is_empty() {
            return self.transform(body, cont);
        }

        let (var, init) = &bindings[0];
        let rest = bindings[1..].to_vec();

        let init_effect = self.effect_ctx.infer(init);

        // Allocate index for this binding and register mapping
        let index = self.next_local_index;
        self.next_local_index += 1;
        self.local_indices.insert(*var, index);

        if init_effect.may_yield() {
            // Yielding initializer - need CPS
            // Transform init with a continuation that binds the result
            let body_clone = body.clone();
            let cont_clone = cont.clone();

            // Create continuation for after init evaluates
            let init_cont = Rc::new(Continuation::LetBinding {
                var: *var,
                remaining_bindings: rest.clone(),
                bound_values: vec![],
                body: Box::new(body_clone),
                next: cont_clone,
            });

            self.transform(init, init_cont)
        } else {
            // Pure initializer - keep as let
            let init_cps = self.transform(init, Continuation::done());
            let body_cps = self.transform_let(&rest, body, cont);

            CpsExpr::Let {
                index,
                init: Box::new(init_cps),
                body: Box::new(body_cps),
            }
        }
    }

    /// Transform internal define (similar to let, but no body - just returns the value)
    fn transform_define(
        &mut self,
        name: SymbolId,
        value: &Expr,
        _cont: Rc<Continuation>,
    ) -> CpsExpr {
        // Allocate index for this binding and register mapping
        let index = self.next_local_index;
        self.next_local_index += 1;
        self.local_indices.insert(name, index);

        // Transform the value expression
        let value_cps = self.transform(value, Continuation::done());

        // Create a let that binds the value and returns it
        // The body just returns the bound value
        CpsExpr::Let {
            index,
            init: Box::new(value_cps),
            body: Box::new(CpsExpr::Var {
                sym: name,
                depth: 0,
                index,
            }),
        }
    }

    /// D4: Transform sequence (begin)
    fn transform_sequence(&mut self, exprs: &[Expr], cont: Rc<Continuation>) -> CpsExpr {
        if exprs.is_empty() {
            return CpsExpr::Literal(Value::Nil);
        }

        if exprs.len() == 1 {
            return self.transform(&exprs[0], cont);
        }

        // Check if any expression may yield
        let any_yields = exprs.iter().any(|e| self.effect_ctx.infer(e).may_yield());

        if !any_yields {
            // All pure - wrap as pure sequence
            CpsExpr::Pure {
                expr: Expr::Begin(exprs.to_vec()),
                continuation: cont,
            }
        } else {
            // Some yield - transform to CPS sequence
            // Each expression is transformed with the outer continuation
            // The interpreter will handle capturing remaining expressions on yield
            let cps_exprs: Vec<CpsExpr> = exprs
                .iter()
                .map(|e| self.transform(e, cont.clone()))
                .collect();

            CpsExpr::Sequence {
                exprs: cps_exprs,
                continuation: cont,
            }
        }
    }

    /// D4: Transform if expression
    fn transform_if(
        &mut self,
        cond: &Expr,
        then_branch: &Expr,
        else_branch: &Expr,
        cont: Rc<Continuation>,
    ) -> CpsExpr {
        let cond_effect = self.effect_ctx.infer(cond);
        let then_effect = self.effect_ctx.infer(then_branch);
        let else_effect = self.effect_ctx.infer(else_branch);

        if cond_effect.is_pure() && then_effect.is_pure() && else_effect.is_pure() {
            // All pure - no transform needed
            CpsExpr::Pure {
                expr: Expr::If {
                    cond: Box::new(cond.clone()),
                    then: Box::new(then_branch.clone()),
                    else_: Box::new(else_branch.clone()),
                },
                continuation: cont,
            }
        } else {
            // Some part yields - transform
            let cond_cps = self.transform(cond, Continuation::done());
            let then_cps = self.transform(then_branch, cont.clone());
            let else_cps = self.transform(else_branch, cont.clone());

            CpsExpr::If {
                cond: Box::new(cond_cps),
                then_branch: Box::new(then_cps),
                else_branch: Box::new(else_cps),
                continuation: cont,
            }
        }
    }

    /// D5: Transform while loop
    fn transform_while(&mut self, cond: &Expr, body: &Expr, cont: Rc<Continuation>) -> CpsExpr {
        let cond_effect = self.effect_ctx.infer(cond);
        let body_effect = self.effect_ctx.infer(body);

        if cond_effect.is_pure() && body_effect.is_pure() {
            // Pure loop - no transform needed
            CpsExpr::Pure {
                expr: Expr::While {
                    cond: Box::new(cond.clone()),
                    body: Box::new(body.clone()),
                },
                continuation: cont,
            }
        } else {
            // Yielding loop - transform
            let cond_cps = self.transform(cond, Continuation::done());
            let body_cps = self.transform(body, Continuation::done());

            CpsExpr::While {
                cond: Box::new(cond_cps),
                body: Box::new(body_cps),
                continuation: cont,
            }
        }
    }

    /// Transform lambda
    fn transform_lambda(
        &mut self,
        params: &[SymbolId],
        body: &Expr,
        captures: &[CaptureInfo],
        num_locals: usize,
        cont: Rc<Continuation>,
    ) -> CpsExpr {
        let num_captures = captures.len();
        let body_effect = self.effect_ctx.infer(body);

        if body_effect.is_pure() {
            // Pure lambda - keep as pure expression
            // The lambda will be compiled to bytecode and executed normally
            CpsExpr::Pure {
                expr: Expr::Lambda {
                    params: params.to_vec(),
                    body: Box::new(body.clone()),
                    captures: captures.to_vec(),
                    num_locals,
                    locals: vec![], // TODO: propagate locals if needed
                },
                continuation: cont,
            }
        } else {
            // Yielding lambda - transform body
            // Save current state
            let saved_index = self.next_local_index;
            let saved_locals = std::mem::take(&mut self.local_indices);

            // New scope: [captures..., params..., locals...]
            self.next_local_index = num_captures + params.len();

            // Register params in local_indices
            for (i, param) in params.iter().enumerate() {
                self.local_indices.insert(*param, num_captures + i);
            }

            let body_cps = self.transform(body, Continuation::done());
            let computed_num_locals = self.next_local_index;

            // Restore state
            self.next_local_index = saved_index;
            self.local_indices = saved_locals;

            // Convert captures from Vec<CaptureInfo> to Vec<(SymbolId, usize, usize)>
            // The tuple format is (symbol, depth, index) - extract from CaptureInfo.source
            let captures_tuple: Vec<(SymbolId, usize, usize)> = captures
                .iter()
                .map(|c| {
                    use crate::binding::VarRef;
                    let index = match &c.source {
                        VarRef::Local { index } | VarRef::Upvalue { index, .. } => *index,
                        VarRef::LetBound { .. } | VarRef::Global { .. } => 0, // Will be loaded from globals
                    };
                    (c.sym, 0, index)
                })
                .collect();

            CpsExpr::Lambda {
                params: params.to_vec(),
                body: Box::new(body_cps),
                captures: captures_tuple,
                num_locals: computed_num_locals,
            }
        }
    }

    /// D4: Transform block expression
    fn transform_block(&mut self, exprs: &[Expr], cont: Rc<Continuation>) -> CpsExpr {
        // Same as sequence but for Block variant
        self.transform_sequence(exprs, cont)
    }

    /// D5: Transform for loop
    fn transform_for(
        &mut self,
        var: SymbolId,
        iter: &Expr,
        body: &Expr,
        cont: Rc<Continuation>,
    ) -> CpsExpr {
        let iter_effect = self.effect_ctx.infer(iter);
        let body_effect = self.effect_ctx.infer(body);

        // Allocate index for loop variable and register mapping
        let index = self.next_local_index;
        self.next_local_index += 1;
        self.local_indices.insert(var, index);

        if iter_effect.is_pure() && body_effect.is_pure() {
            // Pure for loop - no transform needed
            // Note: We still allocate the index but the Pure wrapper handles it
            CpsExpr::Pure {
                expr: Expr::For {
                    var,
                    iter: Box::new(iter.clone()),
                    body: Box::new(body.clone()),
                },
                continuation: cont,
            }
        } else {
            // Yielding for loop - transform
            let iter_cps = self.transform(iter, Continuation::done());
            let body_cps = self.transform(body, Continuation::done());

            CpsExpr::For {
                index,
                iter: Box::new(iter_cps),
                body: Box::new(body_cps),
                continuation: cont,
            }
        }
    }

    /// Transform and expression
    fn transform_and(&mut self, exprs: &[Expr], cont: Rc<Continuation>) -> CpsExpr {
        let any_yields = exprs.iter().any(|e| self.effect_ctx.infer(e).may_yield());

        if !any_yields {
            CpsExpr::Pure {
                expr: Expr::And(exprs.to_vec()),
                continuation: cont,
            }
        } else {
            // Transform each expression
            let mut cps_exprs = Vec::with_capacity(exprs.len());
            for e in exprs {
                cps_exprs.push(self.transform(e, Continuation::done()));
            }

            CpsExpr::And {
                exprs: cps_exprs,
                continuation: cont,
            }
        }
    }

    /// Transform or expression
    fn transform_or(&mut self, exprs: &[Expr], cont: Rc<Continuation>) -> CpsExpr {
        let any_yields = exprs.iter().any(|e| self.effect_ctx.infer(e).may_yield());

        if !any_yields {
            CpsExpr::Pure {
                expr: Expr::Or(exprs.to_vec()),
                continuation: cont,
            }
        } else {
            let mut cps_exprs = Vec::with_capacity(exprs.len());
            for e in exprs {
                cps_exprs.push(self.transform(e, Continuation::done()));
            }

            CpsExpr::Or {
                exprs: cps_exprs,
                continuation: cont,
            }
        }
    }

    /// Transform cond expression
    fn transform_cond(
        &mut self,
        clauses: &[(Expr, Expr)],
        else_body: Option<&Expr>,
        cont: Rc<Continuation>,
    ) -> CpsExpr {
        let any_yields = clauses.iter().any(|(c, b)| {
            self.effect_ctx.infer(c).may_yield() || self.effect_ctx.infer(b).may_yield()
        }) || else_body
            .map(|e| self.effect_ctx.infer(e).may_yield())
            .unwrap_or(false);

        if !any_yields {
            CpsExpr::Pure {
                expr: Expr::Cond {
                    clauses: clauses.to_vec(),
                    else_body: else_body.map(|e| Box::new(e.clone())),
                },
                continuation: cont,
            }
        } else {
            let mut cps_clauses = Vec::with_capacity(clauses.len());
            for (c, b) in clauses {
                let cond_cps = self.transform(c, Continuation::done());
                let body_cps = self.transform(b, cont.clone());
                cps_clauses.push((cond_cps, body_cps));
            }

            let cps_else = else_body.map(|e| Box::new(self.transform(e, cont.clone())));

            CpsExpr::Cond {
                clauses: cps_clauses,
                else_body: cps_else,
                continuation: cont,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol::SymbolTable;

    fn make_ctx() -> EffectContext {
        EffectContext::new()
    }

    #[test]
    fn test_transform_literal() {
        let ctx = make_ctx();
        let mut transformer = CpsTransformer::new(&ctx);

        let expr = Expr::Literal(Value::Int(42));
        let result = transformer.transform(&expr, Continuation::done());

        assert!(result.is_pure());
    }

    #[test]
    fn test_transform_yield() {
        let ctx = make_ctx();
        let mut transformer = CpsTransformer::new(&ctx);

        let expr = Expr::Yield(Box::new(Expr::Literal(Value::Int(1))));
        let result = transformer.transform(&expr, Continuation::done());

        assert!(result.is_yield());
    }

    #[test]
    fn test_transform_pure_call() {
        let mut symbols = SymbolTable::new();
        let plus = symbols.intern("+");
        let ctx = EffectContext::with_symbols(&symbols);
        let mut transformer = CpsTransformer::new(&ctx);

        let expr = Expr::Call {
            func: Box::new(Expr::Var(crate::binding::VarRef::global(plus))),
            args: vec![Expr::Literal(Value::Int(1)), Expr::Literal(Value::Int(2))],
            tail: false,
        };
        let result = transformer.transform(&expr, Continuation::done());

        // Pure call should use PureCall
        match result {
            CpsExpr::PureCall { .. } => {}
            _ => panic!("Expected PureCall, got {:?}", result),
        }
    }

    #[test]
    fn test_transform_pure_if() {
        let ctx = make_ctx();
        let mut transformer = CpsTransformer::new(&ctx);

        let expr = Expr::If {
            cond: Box::new(Expr::Literal(Value::Bool(true))),
            then: Box::new(Expr::Literal(Value::Int(1))),
            else_: Box::new(Expr::Literal(Value::Int(2))),
        };
        let result = transformer.transform(&expr, Continuation::done());

        assert!(result.is_pure());
    }

    #[test]
    fn test_transform_yielding_if() {
        let ctx = make_ctx();
        let mut transformer = CpsTransformer::new(&ctx);

        let expr = Expr::If {
            cond: Box::new(Expr::Literal(Value::Bool(true))),
            then: Box::new(Expr::Yield(Box::new(Expr::Literal(Value::Int(1))))),
            else_: Box::new(Expr::Literal(Value::Int(2))),
        };
        let result = transformer.transform(&expr, Continuation::done());

        // Should be CpsExpr::If, not Pure
        match result {
            CpsExpr::If { .. } => {}
            _ => panic!("Expected If, got {:?}", result),
        }
    }

    #[test]
    fn test_transform_pure_for() {
        let mut symbols = SymbolTable::new();
        let x = symbols.intern("x");
        let ctx = EffectContext::with_symbols(&symbols);
        let mut transformer = CpsTransformer::new(&ctx);

        let expr = Expr::For {
            var: x,
            iter: Box::new(Expr::Literal(Value::Nil)),
            body: Box::new(Expr::Literal(Value::Int(1))),
        };
        let result = transformer.transform(&expr, Continuation::done());

        assert!(result.is_pure());
    }

    #[test]
    fn test_transform_yielding_for() {
        let mut symbols = SymbolTable::new();
        let x = symbols.intern("x");
        let ctx = EffectContext::with_symbols(&symbols);
        let mut transformer = CpsTransformer::new(&ctx);

        let expr = Expr::For {
            var: x,
            iter: Box::new(Expr::Literal(Value::Nil)),
            body: Box::new(Expr::Yield(Box::new(Expr::Literal(Value::Int(1))))),
        };
        let result = transformer.transform(&expr, Continuation::done());

        // Should be CpsExpr::For, not Pure
        match result {
            CpsExpr::For { .. } => {}
            _ => panic!("Expected For, got {:?}", result),
        }
    }

    #[test]
    fn test_transform_pure_and() {
        let ctx = make_ctx();
        let mut transformer = CpsTransformer::new(&ctx);

        let expr = Expr::And(vec![
            Expr::Literal(Value::Bool(true)),
            Expr::Literal(Value::Bool(false)),
        ]);
        let result = transformer.transform(&expr, Continuation::done());

        assert!(result.is_pure());
    }

    #[test]
    fn test_transform_yielding_and() {
        let ctx = make_ctx();
        let mut transformer = CpsTransformer::new(&ctx);

        let expr = Expr::And(vec![
            Expr::Literal(Value::Bool(true)),
            Expr::Yield(Box::new(Expr::Literal(Value::Bool(false)))),
        ]);
        let result = transformer.transform(&expr, Continuation::done());

        // Should be CpsExpr::And, not Pure
        match result {
            CpsExpr::And { .. } => {}
            _ => panic!("Expected And, got {:?}", result),
        }
    }

    #[test]
    fn test_transform_pure_or() {
        let ctx = make_ctx();
        let mut transformer = CpsTransformer::new(&ctx);

        let expr = Expr::Or(vec![
            Expr::Literal(Value::Bool(true)),
            Expr::Literal(Value::Bool(false)),
        ]);
        let result = transformer.transform(&expr, Continuation::done());

        assert!(result.is_pure());
    }

    #[test]
    fn test_transform_yielding_or() {
        let ctx = make_ctx();
        let mut transformer = CpsTransformer::new(&ctx);

        let expr = Expr::Or(vec![
            Expr::Literal(Value::Bool(true)),
            Expr::Yield(Box::new(Expr::Literal(Value::Bool(false)))),
        ]);
        let result = transformer.transform(&expr, Continuation::done());

        // Should be CpsExpr::Or, not Pure
        match result {
            CpsExpr::Or { .. } => {}
            _ => panic!("Expected Or, got {:?}", result),
        }
    }

    #[test]
    fn test_transform_pure_cond() {
        let ctx = make_ctx();
        let mut transformer = CpsTransformer::new(&ctx);

        let expr = Expr::Cond {
            clauses: vec![(
                Expr::Literal(Value::Bool(true)),
                Expr::Literal(Value::Int(1)),
            )],
            else_body: Some(Box::new(Expr::Literal(Value::Int(2)))),
        };
        let result = transformer.transform(&expr, Continuation::done());

        assert!(result.is_pure());
    }

    #[test]
    fn test_transform_yielding_cond() {
        let ctx = make_ctx();
        let mut transformer = CpsTransformer::new(&ctx);

        let expr = Expr::Cond {
            clauses: vec![(
                Expr::Literal(Value::Bool(true)),
                Expr::Yield(Box::new(Expr::Literal(Value::Int(1)))),
            )],
            else_body: Some(Box::new(Expr::Literal(Value::Int(2)))),
        };
        let result = transformer.transform(&expr, Continuation::done());

        // Should be CpsExpr::Cond, not Pure
        match result {
            CpsExpr::Cond { .. } => {}
            _ => panic!("Expected Cond, got {:?}", result),
        }
    }

    #[test]
    fn test_transform_block() {
        let ctx = make_ctx();
        let mut transformer = CpsTransformer::new(&ctx);

        let expr = Expr::Block(vec![
            Expr::Literal(Value::Int(1)),
            Expr::Literal(Value::Int(2)),
        ]);
        let result = transformer.transform(&expr, Continuation::done());

        assert!(result.is_pure());
    }

    #[test]
    fn test_transform_yielding_block() {
        let ctx = make_ctx();
        let mut transformer = CpsTransformer::new(&ctx);

        let expr = Expr::Block(vec![
            Expr::Literal(Value::Int(1)),
            Expr::Yield(Box::new(Expr::Literal(Value::Int(2)))),
        ]);
        let result = transformer.transform(&expr, Continuation::done());

        // Should be CpsExpr::Sequence, not Pure
        match result {
            CpsExpr::Sequence { .. } => {}
            _ => panic!("Expected Sequence, got {:?}", result),
        }
    }
}
