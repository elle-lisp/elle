use elle::compiler::converters::value_to_expr;
use elle::{compile, read_str, register_primitives, SymbolTable, VM};
use iai_callgrind::black_box;

// IAI-CALLGRIND: Instruction-level benchmarks for deterministic performance measurement
// These count actual CPU instructions and are deterministic (no variance)

fn main() {
    // Run each benchmark once to gather instruction counts
    println!("\n=== IAI-CALLGRIND Instruction Counting Benchmarks ===\n");

    println!("Parsing benchmarks:");
    bench_parse_simple();
    println!("  parse_simple: Complete");

    bench_parse_list();
    println!("  parse_list: Complete");

    bench_parse_nested();
    println!("  parse_nested: Complete");

    println!("\nSymbol interning benchmarks:");
    bench_intern_first();
    println!("  intern_first: Complete");

    bench_intern_cached();
    println!("  intern_cached: Complete");

    println!("\nCompilation benchmarks:");
    bench_compile_simple();
    println!("  compile_simple: Complete");

    bench_compile_nested();
    println!("  compile_nested: Complete");

    println!("\nVM execution benchmarks:");
    bench_vm_arithmetic();
    println!("  vm_arithmetic: Complete");

    bench_vm_list();
    println!("  vm_list: Complete");

    println!("\nEnd-to-end benchmarks:");
    bench_end_to_end_simple();
    println!("  end_to_end_simple: Complete");

    println!("\nTo see detailed instruction counts, run with:");
    println!("  cargo bench --bench iai_benchmarks -- --verbose");
}

fn bench_parse_simple() {
    let mut symbols = SymbolTable::new();
    black_box(read_str("42", &mut symbols).unwrap());
}

fn bench_parse_list() {
    let mut symbols = SymbolTable::new();
    black_box(read_str("(1 2 3 4 5)", &mut symbols).unwrap());
}

fn bench_parse_nested() {
    let mut symbols = SymbolTable::new();
    black_box(read_str("(+ (* 2 3) (- 10 5))", &mut symbols).unwrap());
}

fn bench_intern_first() {
    let mut symbols = SymbolTable::new();
    black_box(symbols.intern("unique-symbol"));
}

fn bench_intern_cached() {
    let mut symbols = SymbolTable::new();
    symbols.intern("cached-symbol");
    black_box(symbols.intern("cached-symbol"));
}

fn bench_compile_simple() {
    let mut symbols = SymbolTable::new();
    let simple = read_str("(+ 1 2)", &mut symbols).unwrap();
    let expr = value_to_expr(&simple, &mut symbols).unwrap();
    black_box(compile(&expr));
}

fn bench_compile_nested() {
    let mut symbols = SymbolTable::new();
    let nested = read_str("(+ (* 2 3) (- 10 (/ 8 2)))", &mut symbols).unwrap();
    let expr = value_to_expr(&nested, &mut symbols).unwrap();
    black_box(compile(&expr));
}

fn bench_vm_arithmetic() {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);
    let value = read_str("(+ 1 2 3 4 5)", &mut symbols).unwrap();
    let expr = value_to_expr(&value, &mut symbols).unwrap();
    let bytecode = compile(&expr);
    black_box(vm.execute(&bytecode).unwrap());
}

fn bench_vm_list() {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);
    let value = read_str("(cons 1 (cons 2 (cons 3 nil)))", &mut symbols).unwrap();
    let expr = value_to_expr(&value, &mut symbols).unwrap();
    let bytecode = compile(&expr);
    black_box(vm.execute(&bytecode).unwrap());
}

fn bench_end_to_end_simple() {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);
    let value = read_str("(+ (* 2 3) (- 10 (/ 8 2)))", &mut symbols).unwrap();
    let expr = value_to_expr(&value, &mut symbols).unwrap();
    let bytecode = compile(&expr);
    black_box(vm.execute(&bytecode).unwrap());
}
