use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use elle::compiler::converters::value_to_expr;
use elle::{compile, read_str, register_primitives, SymbolTable, VM};

// DEFENSE: Basic compilation speed for expressions
fn bench_interpreter_compilation(c: &mut Criterion) {
    let mut group = c.benchmark_group("interpreter_compilation");
    let mut symbols = SymbolTable::new();

    // Simple arithmetic - minimal overhead
    group.bench_function("simple_add", |b| {
        b.iter(|| {
            let value = black_box(read_str("(+ 1 2)", &mut symbols).unwrap());
            let expr = value_to_expr(&value, &mut symbols).unwrap();
            black_box(compile(&expr))
        });
    });

    // Conditional - requires branching logic
    group.bench_function("conditional", |b| {
        b.iter(|| {
            let value = black_box(read_str("(if (> 5 3) 100 200)", &mut symbols).unwrap());
            let expr = value_to_expr(&value, &mut symbols).unwrap();
            black_box(compile(&expr))
        });
    });

    // Nested arithmetic - tests expression tree handling
    group.bench_function("nested_arithmetic", |b| {
        b.iter(|| {
            let value = black_box(read_str("(+ (* 2 3) (- 10 (/ 8 2)))", &mut symbols).unwrap());
            let expr = value_to_expr(&value, &mut symbols).unwrap();
            black_box(compile(&expr))
        });
    });

    group.finish();
}

// DEFENSE: Interpreter execution for different expression types
fn bench_interpreter_execution(c: &mut Criterion) {
    let mut group = c.benchmark_group("interpreter_execution");

    let test_cases = vec![
        ("add", "(+ 1 2 3 4 5)"),
        ("multiply", "(* 2 3 4 5)"),
        ("mixed_arithmetic", "(+ (* 2 3) (- 10 (/ 8 2)))"),
        ("comparison_simple", "(< 5 10)"),
        ("comparison_complex", "(and (> 10 5) (< 3 7))"),
    ];

    for (name, expr_str) in test_cases {
        group.bench_with_input(
            BenchmarkId::new("execute", name),
            &expr_str,
            |b, &expr_str| {
                let mut vm = VM::new();
                let mut symbols = SymbolTable::new();
                register_primitives(&mut vm, &mut symbols);
                let value = read_str(expr_str, &mut symbols).unwrap();
                let expr = value_to_expr(&value, &mut symbols).unwrap();
                let bytecode = compile(&expr);
                b.iter(|| black_box(vm.execute(&bytecode).unwrap()));
            },
        );
    }

    group.finish();
}

// DEFENSE: Compilation + execution combined
fn bench_parse_compile_execute(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_compile_execute");

    let benchmarks = vec![
        ("simple", "(+ 1 2 3)"),
        ("arithmetic", "(+ (* 2 3) (- 10 5))"),
        ("conditional", "(if (> 10 5) 100 200)"),
        ("complex", "(if (> (+ 2 3) (- 10 5)) (* 20 30) (/ 100 10))"),
    ];

    for (name, expr_str) in benchmarks {
        group.bench_with_input(
            BenchmarkId::new("pipeline", name),
            &expr_str,
            |b, &expr_str| {
                let mut vm = VM::new();
                let mut symbols = SymbolTable::new();
                register_primitives(&mut vm, &mut symbols);

                b.iter(|| {
                    let value = black_box(read_str(expr_str, &mut symbols).unwrap());
                    let expr = value_to_expr(&value, &mut symbols).unwrap();
                    let bytecode = compile(&expr);
                    black_box(vm.execute(&bytecode).unwrap())
                });
            },
        );
    }

    group.finish();
}

// DEFENSE: Benchmark different expression types
fn bench_expression_types(c: &mut Criterion) {
    let mut group = c.benchmark_group("expression_types");
    let mut symbols = SymbolTable::new();

    let test_cases = vec![
        ("literal_int", "42"),
        ("literal_float", "3.14"),
        ("literal_bool", "true"),
        ("list_literal", "(1 2 3)"),
        ("simple_add", "(+ 1 2)"),
        ("simple_mul", "(* 5 6)"),
        ("comparison_lt", "(< 5 10)"),
        ("comparison_gt", "(> 20 15)"),
        ("comparison_eq", "(= 7 7)"),
        ("mixed_operations", "(+ 1 (* 2 3) (- 5 2))"),
    ];

    for (name, expr_str) in test_cases {
        group.bench_with_input(
            BenchmarkId::new("compile", name),
            &expr_str,
            |b, &expr_str| {
                let value = read_str(expr_str, &mut symbols).unwrap();
                let expr = value_to_expr(&value, &mut symbols).unwrap();
                b.iter(|| black_box(compile(&expr)));
            },
        );
    }

    group.finish();
}

// DEFENSE: Compilation scalability with expression complexity
fn bench_scalability(c: &mut Criterion) {
    let mut group = c.benchmark_group("scalability");
    let mut symbols = SymbolTable::new();

    // Increasing depth of nesting
    for depth in [2, 5, 10, 15].iter() {
        group.bench_with_input(
            BenchmarkId::new("nesting_depth", depth),
            depth,
            |b, &depth| {
                // Create nested expression: (+ (+ (+ ... 1)))
                let mut expr_str = String::from("1");
                for _ in 0..depth {
                    expr_str = format!("(+ {})", expr_str);
                }
                let value = read_str(&expr_str, &mut symbols).unwrap();
                let expr = value_to_expr(&value, &mut symbols).unwrap();
                b.iter(|| black_box(compile(&expr)));
            },
        );
    }

    // Increasing number of operations
    for count in [5, 10, 25, 50].iter() {
        group.bench_with_input(
            BenchmarkId::new("operation_count", count),
            count,
            |b, &count| {
                // Create expression with many operations: (+ 1 2 3 4 5 ...)
                let expr_str = format!(
                    "(+ {})",
                    (1..=count)
                        .map(|i| i.to_string())
                        .collect::<Vec<_>>()
                        .join(" ")
                );
                let value = read_str(&expr_str, &mut symbols).unwrap();
                let expr = value_to_expr(&value, &mut symbols).unwrap();
                b.iter(|| black_box(compile(&expr)));
            },
        );
    }

    group.finish();
}

// DEFENSE: Primitive type handling performance
fn bench_primitives(c: &mut Criterion) {
    let mut group = c.benchmark_group("primitives");
    let mut symbols = SymbolTable::new();

    let primitives = vec![
        ("int_zero", "0"),
        ("int_positive", "42"),
        ("int_large", "999999"),
        ("float_simple", "3.14"),
        ("float_zero", "0.0"),
        ("bool_true", "true"),
        ("bool_false", "false"),
        ("nil", "nil"),
    ];

    for (name, value_str) in primitives {
        group.bench_with_input(
            BenchmarkId::new("compile", name),
            &value_str,
            |b, &value_str| {
                let value = read_str(value_str, &mut symbols).unwrap();
                let expr = value_to_expr(&value, &mut symbols).unwrap();
                b.iter(|| black_box(compile(&expr)));
            },
        );
    }

    group.finish();
}

// DEFENSE: Binary operations performance
fn bench_binary_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("binary_ops");
    let mut symbols = SymbolTable::new();

    let operations = vec![
        ("add", "(+ 5 3)"),
        ("sub", "(- 10 4)"),
        ("mul", "(* 6 7)"),
        ("div", "(/ 20 4)"),
        ("lt", "(< 5 10)"),
        ("gt", "(> 20 5)"),
        ("eq", "(= 7 7)"),
        ("lte", "(<= 5 10)"),
        ("gte", "(>= 20 5)"),
        ("neq", "(!= 3 5)"),
    ];

    for (name, expr_str) in operations {
        group.bench_with_input(
            BenchmarkId::new("compile", name),
            &expr_str,
            |b, &expr_str| {
                let value = read_str(expr_str, &mut symbols).unwrap();
                let expr = value_to_expr(&value, &mut symbols).unwrap();
                b.iter(|| black_box(compile(&expr)));
            },
        );
    }

    group.finish();
}

// DEFENSE: Control flow compilation
fn bench_control_flow(c: &mut Criterion) {
    let mut group = c.benchmark_group("control_flow");
    let mut symbols = SymbolTable::new();

    let flows = vec![
        ("simple_if", "(if true 1 2)"),
        ("if_with_condition", "(if (> 5 3) 100 200)"),
        ("nested_if_2", "(if (> 5 3) (if (< 2 4) 1 2) 3)"),
        ("nested_if_3", "(if true (if true (if true 1 2) 3) 4)"),
        ("if_with_ops", "(if (> (+ 2 3) 4) (* 10 20) (- 100 50))"),
    ];

    for (name, expr_str) in flows {
        group.bench_with_input(
            BenchmarkId::new("compile", name),
            &expr_str,
            |b, &expr_str| {
                let value = read_str(expr_str, &mut symbols).unwrap();
                let expr = value_to_expr(&value, &mut symbols).unwrap();
                b.iter(|| black_box(compile(&expr)));
            },
        );
    }

    group.finish();
}

// DEFENSE: Symbol table performance
fn bench_symbol_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("symbol_operations");

    // First intern (hash + insert)
    group.bench_function("first_intern", |b| {
        b.iter_batched(
            SymbolTable::new,
            |mut symbols| black_box(symbols.intern("unique-symbol")),
            criterion::BatchSize::SmallInput,
        );
    });

    // Repeat intern (hash lookup only)
    group.bench_function("repeat_intern", |b| {
        let mut symbols = SymbolTable::new();
        symbols.intern("cached-symbol");
        b.iter(|| black_box(symbols.intern("cached-symbol")));
    });

    // Many unique symbols
    group.bench_function("many_unique", |b| {
        b.iter_batched(
            SymbolTable::new,
            |mut symbols| {
                for i in 0..100 {
                    black_box(symbols.intern(&format!("symbol-{}", i)));
                }
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

// DEFENSE: VM execution performance for different operation counts
fn bench_vm_scalability(c: &mut Criterion) {
    let mut group = c.benchmark_group("vm_scalability");

    for count in [5, 10, 25, 50, 100].iter() {
        group.bench_with_input(BenchmarkId::new("add_chain", count), count, |b, &count| {
            let mut vm = VM::new();
            let mut symbols = SymbolTable::new();
            register_primitives(&mut vm, &mut symbols);

            let expr_str = format!(
                "(+ {})",
                (0..count)
                    .map(|i| i.to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            let value = read_str(&expr_str, &mut symbols).unwrap();
            let expr = value_to_expr(&value, &mut symbols).unwrap();
            let bytecode = compile(&expr);

            b.iter(|| black_box(vm.execute(&bytecode).unwrap()));
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_interpreter_compilation,
    bench_interpreter_execution,
    bench_parse_compile_execute,
    bench_expression_types,
    bench_scalability,
    bench_primitives,
    bench_binary_operations,
    bench_control_flow,
    bench_symbol_operations,
    bench_vm_scalability,
);

criterion_main!(benches);
