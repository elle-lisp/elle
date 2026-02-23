// Debug test for printing LIR structure
use elle::hir::Analyzer;
use elle::lir::Lowerer;
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
fn test_print_lir_failing_case() {
    let (mut symbols, mut vm) = setup();

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
    let expanded = expander
        .expand(syntax, &mut symbols, &mut vm)
        .expect("expand failed");

    // Analyze
    let mut analyzer = Analyzer::new(&mut symbols);
    let analysis = analyzer.analyze(&expanded).expect("analyze failed");

    // Lower
    let mut lowerer = Lowerer::new().with_bindings(analysis.bindings);
    let lir = lowerer.lower(&analysis.hir).expect("lower failed");

    println!("=== LIR MAIN FUNCTION ===");
    println!("name: {:?}", lir.name);
    println!("arity: {}", lir.arity);
    println!("num_locals: {}", lir.num_locals);
    println!("num_regs: {}", lir.num_regs);
    println!("entry: {:?}", lir.entry);
    println!("cell_params_mask: 0x{:x}", lir.cell_params_mask);

    println!("\n=== BLOCKS ({}) ===", lir.blocks.len());
    for block in &lir.blocks {
        println!("\nBlock {:?}:", block.label);
        println!("  Instructions ({}):", block.instructions.len());
        for instr in &block.instructions {
            println!("    {:?}", instr);
        }
        println!("  Terminator: {:?}", block.terminator);
    }

    println!("\n=== CONSTANTS ({}) ===", lir.constants.len());
    for (i, c) in lir.constants.iter().enumerate() {
        println!("  [{}] = {:?}", i, c);
    }

    // Print nested functions info
    println!("\n=== NESTED FUNCTIONS ===");
    fn count_nested(func: &elle::lir::LirFunction) -> usize {
        let mut count = 0;
        for block in &func.blocks {
            for spanned in &block.instructions {
                if let elle::lir::LirInstr::MakeClosure { func: nested, .. } = &spanned.instr {
                    count += 1 + count_nested(nested);
                }
            }
        }
        count
    }
    let nested_count = count_nested(&lir);
    println!("Total nested functions: {}", nested_count);
}
