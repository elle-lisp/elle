//! Def-use chains and value origin analysis for functional HIR.
//!
//! Single forward pass over the HIR tree builds:
//! - `def_site`: where each binding is defined (HirId)
//! - `uses`: where each binding is used (`Vec<HirId>`)
//! - `value_origin`: what each result-position expression produces

use super::binding::Binding;
use super::expr::{Hir, HirId, HirKind};

use std::collections::HashMap;

/// What a result-position expression produces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueOrigin {
    /// nil, bool, int, float, keyword, empty-list
    Immediate,
    /// Reference to another binding
    Binding(Binding),
    /// Opaque function call result
    CallResult,
    /// Lambda, string literal, quote, MakeCell
    Allocation,
    /// deref-cell (value unknown statically)
    CellDeref,
    /// Control-flow merge with different origins
    Mixed,
}

impl ValueOrigin {
    fn merge(a: &ValueOrigin, b: &ValueOrigin) -> ValueOrigin {
        if a == b {
            a.clone()
        } else {
            ValueOrigin::Mixed
        }
    }

    /// Fold an iterator of origins into a single merged origin.
    fn fold(origins: impl Iterator<Item = ValueOrigin>) -> ValueOrigin {
        let mut result: Option<ValueOrigin> = None;
        for o in origins {
            result = Some(match result {
                None => o,
                Some(prev) => Self::merge(&prev, &o),
            });
        }
        result.unwrap_or(ValueOrigin::Immediate)
    }
}

/// Def-use chain builder. Accumulates results during the walk.
pub(crate) struct DefUseBuilder {
    pub def_site: HashMap<Binding, HirId>,
    pub uses: HashMap<Binding, Vec<HirId>>,
    pub value_origin: HashMap<HirId, ValueOrigin>,
}

impl DefUseBuilder {
    pub fn new() -> Self {
        DefUseBuilder {
            def_site: HashMap::new(),
            uses: HashMap::new(),
            value_origin: HashMap::new(),
        }
    }

    fn record_def(&mut self, binding: Binding, hir_id: HirId) {
        self.def_site.insert(binding, hir_id);
    }

    fn record_use(&mut self, binding: Binding, hir_id: HirId) {
        self.uses.entry(binding).or_default().push(hir_id);
    }

    fn record_origin(&mut self, hir_id: HirId, origin: ValueOrigin) {
        self.value_origin.insert(hir_id, origin);
    }

    fn origin_of(&self, id: HirId) -> ValueOrigin {
        self.value_origin
            .get(&id)
            .cloned()
            .unwrap_or(ValueOrigin::Mixed)
    }

    /// Walk a HIR node, building def-use chains and value origins.
    pub fn walk(&mut self, hir: &Hir) {
        let origin = self.compute(hir);
        self.record_origin(hir.id, origin);
    }

    /// Compute value origin for a node, recording defs/uses along the way.
    fn compute(&mut self, hir: &Hir) -> ValueOrigin {
        match &hir.kind {
            // Literals
            HirKind::Nil
            | HirKind::EmptyList
            | HirKind::Bool(_)
            | HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::Keyword(_) => ValueOrigin::Immediate,

            // Allocations
            HirKind::String(_) | HirKind::Quote(_) => ValueOrigin::Allocation,
            HirKind::Lambda {
                params,
                rest_param,
                captures,
                body,
                ..
            } => {
                // Record parameter defs
                for p in params {
                    self.record_def(*p, hir.id);
                }
                if let Some(rp) = rest_param {
                    self.record_def(*rp, hir.id);
                }
                // Captures generate uses of outer bindings at the lambda's HirId
                for cap in captures {
                    self.record_use(cap.binding, hir.id);
                }
                // Walk body recursively
                self.walk(body);
                ValueOrigin::Allocation
            }

            HirKind::MakeCell { value } => {
                self.walk(value);
                ValueOrigin::Allocation
            }

            // Variable reference
            HirKind::Var(b) => {
                self.record_use(*b, hir.id);
                ValueOrigin::Binding(*b)
            }

            // Cell deref
            HirKind::DerefCell { cell } => {
                self.walk(cell);
                ValueOrigin::CellDeref
            }

            // SetCell: use of cell + value; returns the written value
            HirKind::SetCell { cell, value } => {
                self.walk(cell);
                self.walk(value);
                // SetCell returns the value written — but we model it conservatively
                self.origin_of(value.id)
            }

            // Call
            HirKind::Call { func, args, .. } => {
                self.walk(func);
                for a in args {
                    self.walk(&a.expr);
                }
                ValueOrigin::CallResult
            }

            // Binding forms
            HirKind::Let { bindings, body } => {
                for (b, init) in bindings {
                    self.walk(init);
                    self.record_def(*b, hir.id);
                }
                self.walk(body);
                self.origin_of(body.id)
            }

            HirKind::Letrec { bindings, body } => {
                for (b, init) in bindings {
                    self.record_def(*b, hir.id);
                    self.walk(init);
                }
                self.walk(body);
                self.origin_of(body.id)
            }

            HirKind::Define { binding, value } => {
                self.walk(value);
                self.record_def(*binding, hir.id);
                self.origin_of(value.id)
            }

            // Control flow
            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.walk(cond);
                self.walk(then_branch);
                self.walk(else_branch);
                ValueOrigin::merge(
                    &self.origin_of(then_branch.id),
                    &self.origin_of(else_branch.id),
                )
            }

            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                for (c, b) in clauses {
                    self.walk(c);
                    self.walk(b);
                }
                if let Some(eb) = else_branch {
                    self.walk(eb);
                }
                let clause_origins = clauses.iter().map(|(_, b)| self.origin_of(b.id));
                let all_origins =
                    clause_origins.chain(else_branch.iter().map(|eb| self.origin_of(eb.id)));
                ValueOrigin::fold(all_origins)
            }

            HirKind::Match { value, arms } => {
                self.walk(value);
                for (pat, guard, body) in arms {
                    for b in pat.bindings().bindings {
                        self.record_def(b, hir.id);
                    }
                    if let Some(g) = guard {
                        self.walk(g);
                    }
                    self.walk(body);
                }
                ValueOrigin::fold(arms.iter().map(|(_, _, body)| self.origin_of(body.id)))
            }

            HirKind::Begin(exprs) => {
                for e in exprs {
                    self.walk(e);
                }
                exprs
                    .last()
                    .map(|last| self.origin_of(last.id))
                    .unwrap_or(ValueOrigin::Immediate)
            }

            HirKind::Block { body, .. } => {
                for e in body {
                    self.walk(e);
                }
                body.last()
                    .map(|last| self.origin_of(last.id))
                    .unwrap_or(ValueOrigin::Immediate)
            }

            HirKind::Break { value, .. } => {
                self.walk(value);
                // Break doesn't produce a value at this position
                ValueOrigin::Immediate
            }

            // Loop/Recur
            HirKind::Loop { bindings, body } => {
                for (b, init) in bindings {
                    self.walk(init);
                    self.record_def(*b, hir.id);
                }
                self.walk(body);
                // Loop result is the body when condition fails (typically nil)
                self.origin_of(body.id)
            }

            HirKind::Recur { args } => {
                for a in args {
                    self.walk(a);
                }
                // Recur doesn't produce a value (jumps back)
                ValueOrigin::Immediate
            }

            // Assign (should be rare after functionalize, but handle structurally)
            HirKind::Assign { target, value } => {
                self.walk(value);
                self.record_def(*target, hir.id);
                self.origin_of(value.id)
            }

            // Boolean short-circuit
            HirKind::And(exprs) | HirKind::Or(exprs) => {
                for e in exprs {
                    self.walk(e);
                }
                ValueOrigin::fold(exprs.iter().map(|e| self.origin_of(e.id)))
            }

            // Emit
            HirKind::Emit { value, .. } => {
                self.walk(value);
                ValueOrigin::Immediate
            }

            // Destructure
            HirKind::Destructure { pattern, value, .. } => {
                self.walk(value);
                for b in pattern.bindings().bindings {
                    self.record_def(b, hir.id);
                }
                ValueOrigin::Immediate
            }

            // Eval
            HirKind::Eval { expr, env } => {
                self.walk(expr);
                self.walk(env);
                ValueOrigin::CallResult
            }

            // Parameterize
            HirKind::Parameterize { bindings, body } => {
                for (k, v) in bindings {
                    self.walk(k);
                    self.walk(v);
                }
                self.walk(body);
                self.origin_of(body.id)
            }

            // While (should be eliminated, but handle structurally)
            HirKind::While { cond, body } => {
                self.walk(cond);
                self.walk(body);
                ValueOrigin::Immediate
            }

            // Intrinsic: walk args; non-allocating → Immediate, allocating → Allocation
            HirKind::Intrinsic { op, args } => {
                for a in args {
                    self.walk(a);
                }
                if op.allocates() {
                    ValueOrigin::Allocation
                } else {
                    ValueOrigin::Immediate
                }
            }

            HirKind::Error => ValueOrigin::Immediate,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hir::dataflow::{analyze_dataflow, DataflowInfo};
    use crate::hir::functionalize::functionalize;
    use crate::hir::tailcall::mark_tail_calls;
    use crate::hir::{Analyzer, BindingArena};
    use crate::primitives::register_primitives;
    use crate::reader::read_syntax;
    use crate::symbol::SymbolTable;
    use crate::syntax::Expander;
    use crate::vm::VM;

    /// Parse → expand → analyze → functionalize → dataflow, returning
    /// everything needed by both def-use and liveness tests.
    fn analyze(source: &str) -> (BindingArena, SymbolTable, DataflowInfo) {
        let mut symbols = SymbolTable::new();
        let mut vm = VM::new();
        let meta = register_primitives(&mut vm, &mut symbols);

        let wrapped = format!(
            "(letrec [cond_var (fn () nil) f (fn (& args) nil) g (fn (& args) nil)] {})",
            source
        );
        let syntax = read_syntax(&wrapped, "<test>").expect("parse failed");
        let mut expander = Expander::new();
        let expanded = expander
            .expand(syntax, &mut symbols, &mut vm)
            .expect("expand failed");
        let mut arena = BindingArena::new();
        let mut analyzer = Analyzer::new(&mut symbols, &mut arena);
        analyzer.bind_primitives(&meta);
        let mut analysis = analyzer.analyze(&expanded).expect("analyze failed");
        mark_tail_calls(&mut analysis.hir);
        functionalize(&mut analysis.hir, &mut arena);

        let info = analyze_dataflow(&analysis.hir);
        (arena, symbols, info)
    }

    /// Find a binding by name in def_site.
    fn find_binding(
        info: &DataflowInfo,
        arena: &BindingArena,
        symbols: &SymbolTable,
        name: &str,
    ) -> Option<Binding> {
        info.def_site
            .keys()
            .find(|&&b| symbols.name(arena.get(b).name) == Some(name))
            .copied()
    }

    fn use_count(info: &DataflowInfo, b: Binding) -> usize {
        info.uses.get(&b).map(|v| v.len()).unwrap_or(0)
    }

    #[test]
    fn test_let_one_def_one_use() {
        let (arena, symbols, info) = analyze("(let [x 1] x)");
        let x = find_binding(&info, &arena, &symbols, "x").expect("x not found");
        assert!(info.def_site.contains_key(&x));
        assert_eq!(use_count(&info, x), 1);
    }

    #[test]
    fn test_let_one_def_two_uses() {
        let (arena, symbols, info) = analyze("(let [x 1] (+ x x))");
        let x = find_binding(&info, &arena, &symbols, "x").expect("x not found");
        assert_eq!(use_count(&info, x), 2);
    }

    #[test]
    fn test_lambda_capture_generates_use() {
        // x used at lambda node (capture) and inside lambda body
        let (arena, symbols, info) = analyze("(let [x 1] (fn () x))");
        let x = find_binding(&info, &arena, &symbols, "x").expect("x not found");
        assert!(use_count(&info, x) >= 1);
    }

    #[test]
    fn test_loop_binding_def_and_use() {
        // while+assign → loop parameter with uses in body+recur
        let (arena, symbols, info) = analyze("(begin (def @i 0) (while (< i 10) (set i (+ i 1))))");
        let i_bindings: Vec<Binding> = info
            .def_site
            .keys()
            .filter(|&&b| symbols.name(arena.get(b).name) == Some("i"))
            .copied()
            .collect();
        assert!(!i_bindings.is_empty());
        let total: usize = i_bindings.iter().map(|&b| use_count(&info, b)).sum();
        assert!(total >= 1, "expected uses of i, got {}", total);
    }

    #[test]
    fn test_value_origin_immediate() {
        let (_, _, info) = analyze("42");
        assert!(info
            .value_origin
            .values()
            .any(|v| *v == ValueOrigin::Immediate));
    }

    #[test]
    fn test_value_origin_call_result() {
        let (_, _, info) = analyze("(f 1)");
        assert!(info
            .value_origin
            .values()
            .any(|v| *v == ValueOrigin::CallResult));
    }

    #[test]
    fn test_value_origin_allocation() {
        let (_, _, info) = analyze("(fn () 1)");
        assert!(info
            .value_origin
            .values()
            .any(|v| *v == ValueOrigin::Allocation));
    }

    #[test]
    fn test_value_origin_mixed() {
        let (_, _, info) = analyze("(if (cond_var) 1 \"hello\")");
        assert!(info.value_origin.values().any(|v| *v == ValueOrigin::Mixed));
    }
}
