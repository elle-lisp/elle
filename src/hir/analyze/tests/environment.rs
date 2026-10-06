// audited: 2026-10-06
//! `(environment)` reifies the bindings in scope in one order, whatever the run.
//!
//! docs/impl/stdlib-cache.md

use super::*;

/// The quoted keys of the `(struct 'k v ...)` call `(environment)` desugars to,
/// in argument order.
fn reified_keys(hir: &Hir) -> Vec<crate::value::SymbolId> {
    let mut node = hir;
    loop {
        match &node.kind {
            HirKind::Lambda { body, .. } => node = body,
            HirKind::Begin(exprs) if !exprs.is_empty() => node = &exprs[exprs.len() - 1],
            HirKind::Call { args, .. } => {
                return args
                    .iter()
                    .step_by(2)
                    .map(|a| match &a.expr.kind {
                        HirKind::Quote(v) => v.as_symbol().expect("a key is a symbol"),
                        other => panic!("a key is a quoted symbol, got {other:?}"),
                    })
                    .collect();
            }
            other => panic!("expected the environment's struct call, got {other:?}"),
        }
    }
}

/// The front end is deterministic: one source compiles to one bytecode, which is
/// what lets the stdlib cache and the bytecode golden stand in for a compile.
/// The counter-factual is the scope's binding map iterated as it lies: every
/// map hashes with its own random keys, so two compiles of one scope order the
/// reified bindings differently, and so emit different bytecode. A sequential
/// `let` opens a scope per binding and hides the order, so the bindings here
/// are one function's parameters, which share a scope.
#[test]
fn environment_reifies_its_bindings_in_one_order_every_compile() {
    let names: Vec<String> = (0..16).map(|i| format!("v{i}")).collect();
    let mut orders = Vec::new();
    for _ in 0..8 {
        let mut symbols = SymbolTable::new();
        let mut arena = BindingArena::new();
        let mut analyzer = Analyzer::new(&mut symbols, &mut arena);
        let params: Vec<Syntax> = names.iter().map(|n| make_symbol(n)).collect();
        let syntax = make_list(vec![
            make_symbol("fn"),
            make_array(params),
            make_list(vec![make_symbol("environment")]),
        ]);
        let result = analyzer.analyze(&syntax).expect("analyzes");
        orders.push(reified_keys(&result.hir));
    }
    assert_eq!(orders[0].len(), names.len(), "every binding is reified");
    for order in &orders[1..] {
        assert_eq!(
            order, &orders[0],
            "two compiles reified in different orders"
        );
    }
}
