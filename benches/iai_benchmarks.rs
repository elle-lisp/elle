use elle::pipeline::{compile_new, eval_new};
use elle::primitives::register_primitives;
use elle::{read_str, SymbolTable, VM};
use iai_callgrind::black_box;

// IAI-CALLGRIND: Instruction-level benchmarks for deterministic performance measurement
// These measure actual CPU instructions executed and are completely deterministic (no variance)
//
// The iai-callgrind crate uses Valgrind's callgrind tool to count:
// - Actual instructions executed (no OS noise)
// - Cache misses and branch mispredictions
// - Function call overhead
// - Memory operations
//
// Run with: cargo bench --bench iai_benchmarks
// Results show instruction count, which is more precise than wall-clock time for:
// - Identifying hot paths
// - Validating compiler optimizations
// - Detecting performance regressions
// - Cross-platform comparisons (no OS variability)
//
// Note: Requires valgrind to be installed:
// Linux: sudo apt-get install valgrind
// macOS: brew install valgrind

fn setup() -> (VM, SymbolTable) {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);
    (vm, symbols)
}

/// Parse a simple number (42)
#[inline(never)]
pub fn bench_parse_simple() {
    let mut symbols = SymbolTable::new();
    black_box(read_str("42", &mut symbols).unwrap());
}

/// Parse a list of 5 numbers
#[inline(never)]
pub fn bench_parse_list() {
    let mut symbols = SymbolTable::new();
    black_box(read_str("(1 2 3 4 5)", &mut symbols).unwrap());
}

/// Parse nested expression with operators
#[inline(never)]
pub fn bench_parse_nested() {
    let mut symbols = SymbolTable::new();
    black_box(read_str("(+ (* 2 3) (- 10 5))", &mut symbols).unwrap());
}

/// Intern a new symbol (first time - forces allocation)
#[inline(never)]
pub fn bench_intern_first() {
    let mut symbols = SymbolTable::new();
    black_box(symbols.intern("unique-symbol"));
}

/// Intern a cached symbol (second time - should be fast)
#[inline(never)]
pub fn bench_intern_cached() {
    let mut symbols = SymbolTable::new();
    symbols.intern("cached-symbol");
    black_box(symbols.intern("cached-symbol"));
}

/// Compile a simple arithmetic expression
#[inline(never)]
pub fn bench_compile_simple() {
    let (_, mut symbols) = setup();
    black_box(compile_new("(+ 1 2)", &mut symbols).unwrap());
}

/// Compile a nested arithmetic expression
#[inline(never)]
pub fn bench_compile_nested() {
    let (_, mut symbols) = setup();
    black_box(compile_new("(+ (* 2 3) (- 10 (/ 8 2)))", &mut symbols).unwrap());
}

/// Execute arithmetic in the VM
#[inline(never)]
pub fn bench_vm_arithmetic() {
    let (mut vm, mut symbols) = setup();
    let result = compile_new("(+ 1 2 3 4 5)", &mut symbols).unwrap();
    black_box(vm.execute(&result.bytecode).unwrap());
}

/// Execute list construction in the VM
#[inline(never)]
pub fn bench_vm_list() {
    let (mut vm, mut symbols) = setup();
    let result = compile_new("(cons 1 (cons 2 (cons 3 nil)))", &mut symbols).unwrap();
    black_box(vm.execute(&result.bytecode).unwrap());
}

/// End-to-end: parse, compile, and execute
#[inline(never)]
pub fn bench_end_to_end_simple() {
    let (mut vm, mut symbols) = setup();
    black_box(eval_new("(+ (* 2 3) (- 10 (/ 8 2)))", &mut symbols, &mut vm).unwrap());
}

fn main() {
    println!("This benchmark file provides deterministic instruction-counting benchmarks.");
    println!("To run: cargo bench --bench iai_benchmarks");
    println!("Requires: valgrind package installed");
}
