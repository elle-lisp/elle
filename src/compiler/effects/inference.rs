//! Effect inference for expressions

use super::Effect;
use crate::compiler::ast::Expr;
use crate::symbol::SymbolTable;
use crate::value::SymbolId;
use std::collections::HashMap;

/// Context for effect inference
pub struct EffectContext {
    /// Known effects of global functions
    known_effects: HashMap<SymbolId, Effect>,
    /// Effects of local variables (for closures passed as arguments)
    local_effects: HashMap<SymbolId, Effect>,
}

impl EffectContext {
    /// Create a new effect context with default primitive effects
    pub fn new() -> Self {
        Self {
            known_effects: HashMap::new(),
            local_effects: HashMap::new(),
        }
    }

    /// Create a new effect context with primitive effects registered from symbol table
    pub fn with_symbols(symbols: &SymbolTable) -> Self {
        let mut ctx = Self::new();
        super::primitives::register_primitive_effects(symbols, &mut ctx.known_effects);
        ctx
    }

    /// Register the effect of a global function
    pub fn register_global(&mut self, sym: SymbolId, effect: Effect) {
        self.known_effects.insert(sym, effect);
    }

    /// Register the effect of a function definition
    pub fn register_function(&mut self, sym: SymbolId, effect: Effect) {
        self.known_effects.insert(sym, effect);
    }

    /// Register the effect of a local variable
    pub fn register_local(&mut self, sym: SymbolId, effect: Effect) {
        self.local_effects.insert(sym, effect);
    }

    /// Look up the effect of a global function
    pub fn get_global(&self, sym: SymbolId) -> Option<Effect> {
        self.known_effects.get(&sym).copied()
    }

    /// Look up the effect of a local variable
    pub fn get_local(&self, sym: SymbolId) -> Option<Effect> {
        self.local_effects.get(&sym).copied()
    }

    /// Infer the effect of an expression
    pub fn infer(&self, expr: &Expr) -> Effect {
        match expr {
            // Literals are always pure
            Expr::Literal(_) => Effect::Pure,

            // Variables are pure (the value itself doesn't yield)
            Expr::Var(var_ref) => {
                use crate::binding::VarRef;
                match var_ref {
                    VarRef::Local { .. } | VarRef::Upvalue { .. } | VarRef::LetBound { .. } => {
                        Effect::Pure
                    }
                    VarRef::Global { sym } => {
                        self.known_effects.get(sym).copied().unwrap_or(Effect::Pure)
                    }
                }
            }

            // Conditionals: max of all branches
            Expr::If { cond, then, else_ } => {
                Effect::combine_all([self.infer(cond), self.infer(then), self.infer(else_)])
            }

            // Let: max of bindings and body
            Expr::Let { bindings, body } => {
                let binding_effects = bindings.iter().map(|(_, e)| self.infer(e));
                let body_effect = self.infer(body);
                Effect::combine_all(binding_effects.chain(std::iter::once(body_effect)))
            }

            // Letrec: max of bindings and body
            Expr::Letrec { bindings, body } => {
                let binding_effects = bindings.iter().map(|(_, e)| self.infer(e));
                let body_effect = self.infer(body);
                Effect::combine_all(binding_effects.chain(std::iter::once(body_effect)))
            }

            // Begin/Block: max of all expressions
            Expr::Begin(exprs) | Expr::Block(exprs) => {
                Effect::combine_all(exprs.iter().map(|e| self.infer(e)))
            }

            // Function call: max of args + effect of callee
            Expr::Call { func, args, .. } => {
                let arg_effects = args.iter().map(|e| self.infer(e));
                let func_effect = self.infer_call_effect(func, args);
                Effect::combine_all(arg_effects.chain(std::iter::once(func_effect)))
            }

            // Lambda: the expression itself is pure
            // (the effect is stored in the closure for when it's called)
            Expr::Lambda { .. } => Effect::Pure,

            // And/Or: max of all expressions
            Expr::And(exprs) | Expr::Or(exprs) | Expr::Xor(exprs) => {
                Effect::combine_all(exprs.iter().map(|e| self.infer(e)))
            }

            // While/For: max of condition and body
            Expr::While { cond, body } => Effect::combine(self.infer(cond), self.infer(body)),
            Expr::For { iter, body, .. } => Effect::combine(self.infer(iter), self.infer(body)),

            // Cond: max of all conditions and bodies
            Expr::Cond { clauses, else_body } => {
                let clause_effects = clauses
                    .iter()
                    .flat_map(|(c, b)| [self.infer(c), self.infer(b)]);
                let else_effect = else_body
                    .as_ref()
                    .map(|e| self.infer(e))
                    .unwrap_or(Effect::Pure);
                Effect::combine_all(clause_effects.chain(std::iter::once(else_effect)))
            }

            // Set: effect of the value expression
            Expr::Set { value, .. } => self.infer(value),

            // Match: max of all patterns and bodies
            Expr::Match {
                value,
                patterns,
                default,
            } => {
                let value_effect = self.infer(value);
                let pattern_effects = patterns.iter().map(|(_, e)| self.infer(e));
                let default_effect = default
                    .as_ref()
                    .map(|e| self.infer(e))
                    .unwrap_or(Effect::Pure);
                Effect::combine_all(
                    std::iter::once(value_effect)
                        .chain(pattern_effects)
                        .chain(std::iter::once(default_effect)),
                )
            }

            // Try: max of body, catch, and finally
            Expr::Try {
                body,
                catch,
                finally,
            } => {
                let body_effect = self.infer(body);
                let catch_effect = catch
                    .as_ref()
                    .map(|(_, e)| self.infer(e))
                    .unwrap_or(Effect::Pure);
                let finally_effect = finally
                    .as_ref()
                    .map(|e| self.infer(e))
                    .unwrap_or(Effect::Pure);
                Effect::combine_all([body_effect, catch_effect, finally_effect])
            }

            // Throw: effect of the value
            Expr::Throw { value } => self.infer(value),

            // Handler-case: max of body and handlers
            Expr::HandlerCase { body, handlers } => {
                let body_effect = self.infer(body);
                let handler_effects = handlers.iter().map(|(_, _, e)| self.infer(e));
                Effect::combine_all(std::iter::once(body_effect).chain(handler_effects))
            }

            // Handler-bind: max of body and handlers
            Expr::HandlerBind { handlers, body } => {
                let body_effect = self.infer(body);
                let handler_effects = handlers.iter().map(|(_, e)| self.infer(e));
                Effect::combine_all(std::iter::once(body_effect).chain(handler_effects))
            }

            // Quote/Quasiquote/Unquote: pure (no evaluation)
            Expr::Quote(_) | Expr::Quasiquote(_) | Expr::Unquote(_) => Effect::Pure,

            // Define/DefMacro/Module/Import/ModuleRef: pure (side effects not tracked)
            Expr::Define { .. }
            | Expr::DefMacro { .. }
            | Expr::Module { .. }
            | Expr::Import { .. }
            | Expr::ModuleRef { .. } => Effect::Pure,

            // Yield: always yields, plus the effect of the yielded expression
            Expr::Yield(expr) => Effect::combine(Effect::Yields, self.infer(expr)),
        }
    }

    /// Infer the effect of calling a function (public for CPS transform)
    pub fn infer_call_effect(&self, func: &Expr, args: &[Expr]) -> Effect {
        use crate::binding::VarRef;
        match func {
            Expr::Var(var_ref) => {
                match var_ref {
                    VarRef::Global { sym } => {
                        match self.known_effects.get(sym) {
                            Some(Effect::Pure) => Effect::Pure,
                            Some(Effect::Yields) => Effect::Yields,
                            Some(Effect::Polymorphic(param_idx)) => {
                                // Effect depends on the param_idx-th argument
                                if let Some(arg) = args.get(*param_idx) {
                                    self.infer_arg_effect(arg)
                                } else {
                                    Effect::Pure // Conservative default
                                }
                            }
                            // Unknown global function - assume pure
                            // At runtime, we register effects from VM globals, so unknown
                            // functions are likely builtins that weren't registered
                            None => Effect::Pure,
                        }
                    }
                    VarRef::Local { .. } | VarRef::Upvalue { .. } | VarRef::LetBound { .. } => {
                        self.local_effects
                            .get(&crate::value::SymbolId(0)) // Placeholder - local vars don't have symbols
                            .copied()
                            .unwrap_or(Effect::Yields) // Unknown local variable, assume may yield (conservative)
                    }
                }
            }
            Expr::Lambda { body, .. } => {
                // Inline lambda - infer its body's effect
                self.infer(body)
            }
            _ => Effect::Yields, // Unknown expression, assume may yield (conservative)
        }
    }

    /// Infer the effect of a function argument (for polymorphic HOFs)
    fn infer_arg_effect(&self, arg: &Expr) -> Effect {
        use crate::binding::VarRef;
        match arg {
            // If arg is a lambda, infer its body effect
            Expr::Lambda { body, .. } => self.infer(body),
            // If arg is a variable, look up its effect
            Expr::Var(var_ref) => {
                match var_ref {
                    VarRef::Global { sym } => {
                        self.known_effects.get(sym).copied().unwrap_or(Effect::Pure)
                    }
                    VarRef::Local { .. } | VarRef::Upvalue { .. } | VarRef::LetBound { .. } => self
                        .local_effects
                        .get(&crate::value::SymbolId(0)) // Placeholder
                        .copied()
                        .unwrap_or(Effect::Pure),
                }
            }
            // Otherwise, the argument expression's effect
            _ => self.infer(arg),
        }
    }

    /// Infer the effect of a lambda body (for storing in closure)
    pub fn infer_lambda_effect(&self, body: &Expr) -> Effect {
        self.infer(body)
    }
}

impl Default for EffectContext {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Value;

    #[test]
    fn test_infer_literal() {
        let ctx = EffectContext::new();
        let expr = Expr::Literal(Value::Int(42));
        assert_eq!(ctx.infer(&expr), Effect::Pure);
    }

    #[test]
    fn test_infer_if() {
        let ctx = EffectContext::new();
        let expr = Expr::If {
            cond: Box::new(Expr::Literal(Value::Bool(true))),
            then: Box::new(Expr::Literal(Value::Int(1))),
            else_: Box::new(Expr::Literal(Value::Int(2))),
        };
        assert_eq!(ctx.infer(&expr), Effect::Pure);
    }

    #[test]
    fn test_infer_begin() {
        let ctx = EffectContext::new();
        let expr = Expr::Begin(vec![
            Expr::Literal(Value::Int(1)),
            Expr::Literal(Value::Int(2)),
        ]);
        assert_eq!(ctx.infer(&expr), Effect::Pure);
    }

    #[test]
    fn test_infer_lambda_effect() {
        let ctx = EffectContext::new();
        // A simple lambda with pure body
        let body = Box::new(Expr::Literal(Value::Int(42)));
        let effect = ctx.infer_lambda_effect(&body);
        assert_eq!(effect, Effect::Pure);
    }

    #[test]
    fn test_register_function() {
        let mut ctx = EffectContext::new();
        let sym = crate::value::SymbolId(1);
        ctx.register_function(sym, Effect::Pure);
        assert_eq!(ctx.get_global(sym), Some(Effect::Pure));
    }

    #[test]
    fn test_with_symbols() {
        let mut symbols = crate::symbol::SymbolTable::new();
        let plus_sym = symbols.intern("+");
        let ctx = EffectContext::with_symbols(&symbols);
        // Should have registered primitive effects
        // (exact effects depend on what primitives are registered)
        assert_eq!(ctx.get_global(plus_sym), Some(Effect::Pure));
    }

    #[test]
    fn test_infer_call_pure_function() {
        let mut ctx = EffectContext::new();
        let sym = crate::value::SymbolId(1);
        ctx.register_global(sym, Effect::Pure);

        let expr = Expr::Call {
            func: Box::new(Expr::Var(crate::binding::VarRef::global(sym))),
            args: vec![Expr::Literal(Value::Int(1))],
            tail: false,
        };
        assert_eq!(ctx.infer(&expr), Effect::Pure);
    }

    #[test]
    fn test_infer_nested_calls() {
        let mut ctx = EffectContext::new();
        let sym1 = crate::value::SymbolId(1);
        let sym2 = crate::value::SymbolId(2);
        ctx.register_global(sym1, Effect::Pure);
        ctx.register_global(sym2, Effect::Pure);

        // (f (g 1))
        let expr = Expr::Call {
            func: Box::new(Expr::Var(crate::binding::VarRef::global(sym1))),
            args: vec![Expr::Call {
                func: Box::new(Expr::Var(crate::binding::VarRef::global(sym2))),
                args: vec![Expr::Literal(Value::Int(1))],
                tail: false,
            }],
            tail: false,
        };
        assert_eq!(ctx.infer(&expr), Effect::Pure);
    }

    #[test]
    fn test_infer_let_binding() {
        let ctx = EffectContext::new();
        let sym = crate::value::SymbolId(1);
        let expr = Expr::Let {
            bindings: vec![(sym, Expr::Literal(Value::Int(1)))],
            body: Box::new(Expr::Literal(Value::Int(2))),
        };
        assert_eq!(ctx.infer(&expr), Effect::Pure);
    }

    #[test]
    fn test_infer_lambda_expression() {
        let ctx = EffectContext::new();
        let expr = Expr::Lambda {
            params: vec![crate::value::SymbolId(1)],
            body: Box::new(Expr::Literal(Value::Int(42))),
            captures: vec![],
            num_locals: 1,
            locals: vec![],
        };
        // Lambda expression itself is pure (the effect is stored in the closure)
        assert_eq!(ctx.infer(&expr), Effect::Pure);
    }

    #[test]
    fn test_polymorphic_map_with_pure_function() {
        let mut symbols = crate::symbol::SymbolTable::new();
        let map_sym = symbols.intern("map");
        let abs_sym = symbols.intern("abs");

        let ctx = EffectContext::with_symbols(&symbols);

        // (map abs lst) - abs is pure, so map is pure
        let expr = Expr::Call {
            func: Box::new(Expr::Var(crate::binding::VarRef::global(map_sym))),
            args: vec![
                Expr::Var(crate::binding::VarRef::global(abs_sym)),
                Expr::Literal(Value::Nil),
            ],
            tail: false,
        };

        assert_eq!(ctx.infer(&expr), Effect::Pure);
    }

    #[test]
    fn test_polymorphic_map_with_inline_lambda() {
        let mut symbols = crate::symbol::SymbolTable::new();
        let map_sym = symbols.intern("map");
        let x_sym = symbols.intern("x");

        let ctx = EffectContext::with_symbols(&symbols);

        // (map (fn (x) (+ x 1)) lst) - inline pure lambda
        let expr = Expr::Call {
            func: Box::new(Expr::Var(crate::binding::VarRef::global(map_sym))),
            args: vec![
                Expr::Lambda {
                    params: vec![x_sym],
                    body: Box::new(Expr::Literal(Value::Int(1))),
                    captures: vec![],
                    num_locals: 1,
                    locals: vec![],
                },
                Expr::Literal(Value::Nil),
            ],
            tail: false,
        };

        assert_eq!(ctx.infer(&expr), Effect::Pure);
    }

    #[test]
    fn test_polymorphic_map_with_yielding_function() {
        let mut symbols = crate::symbol::SymbolTable::new();
        let map_sym = symbols.intern("map");
        let yielding_fn_sym = symbols.intern("yielding-fn");

        let mut ctx = EffectContext::with_symbols(&symbols);
        ctx.register_global(yielding_fn_sym, Effect::Yields);

        // (map yielding-fn lst) - yielding-fn yields, so map yields
        let expr = Expr::Call {
            func: Box::new(Expr::Var(crate::binding::VarRef::global(map_sym))),
            args: vec![
                Expr::Var(crate::binding::VarRef::global(yielding_fn_sym)),
                Expr::Literal(Value::Nil),
            ],
            tail: false,
        };

        assert_eq!(ctx.infer(&expr), Effect::Yields);
    }

    #[test]
    fn test_polymorphic_map_with_yielding_lambda() {
        let mut symbols = crate::symbol::SymbolTable::new();
        let map_sym = symbols.intern("map");
        let yield_sym = symbols.intern("yield");
        let x_sym = symbols.intern("x");

        let mut ctx = EffectContext::with_symbols(&symbols);
        ctx.register_global(yield_sym, Effect::Yields);

        // (map (fn (x) (yield x)) lst) - lambda body yields
        let expr = Expr::Call {
            func: Box::new(Expr::Var(crate::binding::VarRef::global(map_sym))),
            args: vec![
                Expr::Lambda {
                    params: vec![x_sym],
                    body: Box::new(Expr::Call {
                        func: Box::new(Expr::Var(crate::binding::VarRef::global(yield_sym))),
                        args: vec![Expr::Var(crate::binding::VarRef::local(0))],
                        tail: false,
                    }),
                    captures: vec![],
                    num_locals: 1,
                    locals: vec![],
                },
                Expr::Literal(Value::Nil),
            ],
            tail: false,
        };

        assert_eq!(ctx.infer(&expr), Effect::Yields);
    }

    #[test]
    fn test_polymorphic_filter_with_pure_function() {
        let mut symbols = crate::symbol::SymbolTable::new();
        let filter_sym = symbols.intern("filter");
        let positive_sym = symbols.intern("positive?");

        let mut ctx = EffectContext::with_symbols(&symbols);
        ctx.register_global(positive_sym, Effect::Pure);

        // (filter positive? lst) - positive? is pure, so filter is pure
        let expr = Expr::Call {
            func: Box::new(Expr::Var(crate::binding::VarRef::global(filter_sym))),
            args: vec![
                Expr::Var(crate::binding::VarRef::global(positive_sym)),
                Expr::Literal(Value::Nil),
            ],
            tail: false,
        };

        assert_eq!(ctx.infer(&expr), Effect::Pure);
    }

    #[test]
    fn test_polymorphic_fold_with_pure_function() {
        let mut symbols = crate::symbol::SymbolTable::new();
        let fold_sym = symbols.intern("fold");
        let plus_sym = symbols.intern("+");

        let ctx = EffectContext::with_symbols(&symbols);

        // (fold + 0 lst) - + is pure, so fold is pure
        let expr = Expr::Call {
            func: Box::new(Expr::Var(crate::binding::VarRef::global(fold_sym))),
            args: vec![
                Expr::Var(crate::binding::VarRef::global(plus_sym)),
                Expr::Literal(Value::Int(0)),
                Expr::Literal(Value::Nil),
            ],
            tail: false,
        };

        assert_eq!(ctx.infer(&expr), Effect::Pure);
    }

    #[test]
    fn test_polymorphic_apply_with_pure_function() {
        let mut symbols = crate::symbol::SymbolTable::new();
        let apply_sym = symbols.intern("apply");
        let plus_sym = symbols.intern("+");

        let ctx = EffectContext::with_symbols(&symbols);

        // (apply + (list 1 2)) - + is pure, so apply is pure
        let expr = Expr::Call {
            func: Box::new(Expr::Var(crate::binding::VarRef::global(apply_sym))),
            args: vec![
                Expr::Var(crate::binding::VarRef::global(plus_sym)),
                Expr::Literal(Value::Nil),
            ],
            tail: false,
        };

        assert_eq!(ctx.infer(&expr), Effect::Pure);
    }

    #[test]
    fn test_polymorphic_with_missing_argument() {
        let mut symbols = crate::symbol::SymbolTable::new();
        let map_sym = symbols.intern("map");

        let ctx = EffectContext::with_symbols(&symbols);

        // (map) - missing argument, should default to pure
        let expr = Expr::Call {
            func: Box::new(Expr::Var(crate::binding::VarRef::global(map_sym))),
            args: vec![],
            tail: false,
        };

        assert_eq!(ctx.infer(&expr), Effect::Pure);
    }

    #[test]
    fn test_infer_arg_effect_with_global_var() {
        let mut symbols = crate::symbol::SymbolTable::new();
        let abs_sym = symbols.intern("abs");

        let ctx = EffectContext::with_symbols(&symbols);

        // Test infer_arg_effect directly
        let arg = Expr::Var(crate::binding::VarRef::global(abs_sym));
        assert_eq!(ctx.infer_arg_effect(&arg), Effect::Pure);
    }

    #[test]
    fn test_infer_arg_effect_with_lambda() {
        let mut symbols = crate::symbol::SymbolTable::new();
        let x_sym = symbols.intern("x");

        let ctx = EffectContext::with_symbols(&symbols);

        // Test infer_arg_effect with a lambda
        let arg = Expr::Lambda {
            params: vec![x_sym],
            body: Box::new(Expr::Literal(Value::Int(42))),
            captures: vec![],
            num_locals: 1,
            locals: vec![],
        };
        assert_eq!(ctx.infer_arg_effect(&arg), Effect::Pure);
    }
}
