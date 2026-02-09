use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use elle::compiler::converters::value_to_expr;
use elle::compiler::JitCoordinator;
use elle::{compile, read_str, register_primitives, SymbolTable, VM};

// Benchmark: JIT Coordinator vs Bytecode for repeated operations
fn bench_coordinator_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("jit_coordinator");

    // Bytecode baseline
    group.bench_function("bytecode_add_1000x", |b| {
        let mut vm = VM::new();
        let mut symbols = SymbolTable::new();
        register_primitives(&mut vm, &mut symbols);

        b.iter(|| {
            for i in 0..1000 {
                let code = format!("(+ {} 1)", i);
                let value = read_str(&code, &mut symbols).unwrap();
                let expr = value_to_expr(&value, &mut symbols).unwrap();
                let bytecode = compile(&expr);
                let _ = vm.execute(&bytecode).unwrap();
            }
        });
    });

    // JIT Coordinator (with profiling overhead)
    group.bench_function("coordinator_add_1000x", |b| {
        let mut vm = VM::new();
        let mut symbols = SymbolTable::new();
        register_primitives(&mut vm, &mut symbols);
        let _coordinator = JitCoordinator::new(true);

        b.iter(|| {
            for i in 0..1000 {
                let code = format!("(+ {} 1)", i);
                let value = read_str(&code, &mut symbols).unwrap();
                let expr = value_to_expr(&value, &mut symbols).unwrap();
                let bytecode = compile(&expr);
                let _ = vm.execute(&bytecode).unwrap();
            }
        });
    });

    group.finish();
}

// Benchmark: Simple arithmetic with various operations
fn bench_arithmetic_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("arithmetic_patterns");

    let operations = vec![
        ("add", "(+ 5 3)"),
        ("multiply", "(* 5 3)"),
        ("subtract", "(- 10 3)"),
        ("divide", "(/ 20 4)"),
        ("mixed", "(+ (* 2 3) (- 10 5))"),
    ];

    for (name, code) in operations {
        group.bench_with_input(BenchmarkId::new("bytecode", name), &code, |b, &code| {
            let mut vm = VM::new();
            let mut s = SymbolTable::new();
            register_primitives(&mut vm, &mut s);

            let value = read_str(code, &mut s).unwrap();
            let expr = value_to_expr(&value, &mut s).unwrap();
            let bytecode = compile(&expr);

            b.iter(|| {
                for _ in 0..100 {
                    let _ = vm.execute(&bytecode).unwrap();
                }
            });
        });
    }

    group.finish();
}

// Benchmark: Conditional expressions
fn bench_conditionals(c: &mut Criterion) {
    let mut group = c.benchmark_group("conditional_performance");

    // Simple if
    group.bench_function("bytecode_if_1000x", |b| {
        let mut vm = VM::new();
        let mut symbols = SymbolTable::new();
        register_primitives(&mut vm, &mut symbols);

        let value = read_str("(if (> 5 3) 100 200)", &mut symbols).unwrap();
        let expr = value_to_expr(&value, &mut symbols).unwrap();
        let bytecode = compile(&expr);

        b.iter(|| {
            for _ in 0..1000 {
                let _ = vm.execute(&bytecode).unwrap();
            }
        });
    });

    // Nested if
    group.bench_function("bytecode_nested_if_1000x", |b| {
        let mut vm = VM::new();
        let mut symbols = SymbolTable::new();
        register_primitives(&mut vm, &mut symbols);

        let value = read_str("(if (> 5 3) (if (< 2 4) 1 2) 3)", &mut symbols).unwrap();
        let expr = value_to_expr(&value, &mut symbols).unwrap();
        let bytecode = compile(&expr);

        b.iter(|| {
            for _ in 0..1000 {
                let _ = vm.execute(&bytecode).unwrap();
            }
        });
    });

    group.finish();
}

// Benchmark: Compilation time
fn bench_compilation_time(c: &mut Criterion) {
    let mut group = c.benchmark_group("compilation");
    let mut symbols = SymbolTable::new();

    let test_cases = vec![
        ("simple_literal", "42"),
        ("simple_add", "(+ 1 2)"),
        ("nested_arithmetic", "(+ (* 2 3) (- 10 5))"),
        ("conditional", "(if (> 5 3) 100 200)"),
        ("complex", "(if (> (+ 2 3) (- 10 5)) (* 20 30) (/ 100 10))"),
    ];

    for (name, code) in test_cases {
        group.bench_with_input(BenchmarkId::new("compile", name), &code, |b, &code| {
            b.iter(|| {
                let value = read_str(code, &mut symbols).unwrap();
                let expr = value_to_expr(&value, &mut symbols).unwrap();
                black_box(compile(&expr))
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_coordinator_overhead,
    bench_arithmetic_operations,
    bench_conditionals,
    bench_compilation_time
);

criterion_main!(benches);
