// Debug test for printing HIR structure
use elle::hir::Analyzer;
use elle::reader::read_syntax;
use elle::symbol::SymbolTable;
use elle::syntax::Expander;
use elle::vm::VM;

fn setup() -> (SymbolTable, VM) {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    let _effects = elle::primitives::register_primitives(&mut vm, &mut symbols);
    (symbols, vm)
}

#[test]
fn test_print_hir_failing_case() {
    let (mut symbols, _vm) = setup();

    let code = r#"(begin
        (define process (fn (acc x) (begin (define doubled (* x 2)) (+ acc doubled))))
        (define my-fold (fn (f init lst)
            (if (nil? lst)
                init
                (my-fold f (f init (first lst)) (rest lst)))))
        (my-fold process 0 (list 1 2)))"#;

    // Parse
    let syntax = read_syntax(code).expect("parse failed");

    // Expand
    let mut expander = Expander::new();
    let expanded = expander.expand(syntax).expect("expand failed");

    // Analyze
    let mut analyzer = Analyzer::new(&mut symbols);
    let analysis = analyzer.analyze(&expanded).expect("analyze failed");

    println!("=== HIR ===");
    println!("{:#?}", analysis.hir);

    println!("\n=== BINDINGS ===");
    for (id, info) in &analysis.bindings {
        println!(
            "  {:?}: name={:?} kind={:?} is_captured={} is_mutated={}",
            id, info.name, info.kind, info.is_captured, info.is_mutated
        );
    }
}
