use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use elle::compiler::converters::value_to_expr;
use elle::{compile, read_str, register_primitives, SymbolTable, VM};

// DEFENSE: Separate parsing from execution to measure each phase independently
fn bench_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("parsing");
    let mut symbols = SymbolTable::new();

    // Simple expression
    group.bench_function("simple_number", |b| {
        b.iter(|| black_box(read_str("42", &mut symbols).unwrap()));
    });

    // List with numbers
    group.bench_function("list_literal", |b| {
        b.iter(|| black_box(read_str("(1 2 3 4 5)", &mut symbols).unwrap()));
    });

    // Nested expression
    group.bench_function("nested_expr", |b| {
        b.iter(|| black_box(read_str("(+ (* 2 3) (- 10 5))", &mut symbols).unwrap()));
    });

    // Deep nesting
    group.bench_function("deep_nesting", |b| {
        b.iter(|| black_box(read_str("(((((1)))))", &mut symbols).unwrap()));
    });

    // Large list
    let large_list = format!(
        "({})",
        (0..100)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(" ")
    );
    group.bench_function("large_list_100", |b| {
        b.iter(|| black_box(read_str(&large_list, &mut symbols).unwrap()));
    });

    group.finish();
}

// DEFENSE: Symbol interning is critical for performance
fn bench_symbol_interning(c: &mut Criterion) {
    let mut group = c.benchmark_group("symbol_interning");

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

// DEFENSE: Compilation speed matters for interactive REPL
fn bench_compilation(c: &mut Criterion) {
    let mut group = c.benchmark_group("compilation");
    let mut symbols = SymbolTable::new();

    // Simple arithmetic
    let simple = read_str("(+ 1 2)", &mut symbols).unwrap();
    group.bench_function("simple_arithmetic", |b| {
        b.iter(|| {
            let expr = value_to_expr(&simple, &mut symbols).unwrap();
            black_box(compile(&expr))
        });
    });

    // Conditional
    let conditional = read_str("(if (> 5 3) 100 200)", &mut symbols).unwrap();
    group.bench_function("conditional", |b| {
        b.iter(|| {
            let expr = value_to_expr(&conditional, &mut symbols).unwrap();
            black_box(compile(&expr))
        });
    });

    // Nested expressions
    let nested = read_str("(+ (* 2 3) (- 10 (/ 8 2)))", &mut symbols).unwrap();
    group.bench_function("nested_arithmetic", |b| {
        b.iter(|| {
            let expr = value_to_expr(&nested, &mut symbols).unwrap();
            black_box(compile(&expr))
        });
    });

    group.finish();
}

// DEFENSE: VM execution is the hot path
fn bench_vm_execution(c: &mut Criterion) {
    let mut group = c.benchmark_group("vm_execution");

    // Integer arithmetic (specialized ops)
    group.bench_function("int_add", |b| {
        let mut vm = VM::new();
        let mut symbols = SymbolTable::new();
        register_primitives(&mut vm, &mut symbols);
        let value = read_str("(+ 1 2 3 4 5)", &mut symbols).unwrap();
        let expr = value_to_expr(&value, &mut symbols).unwrap();
        let bytecode = compile(&expr);
        b.iter(|| black_box(vm.execute(&bytecode).unwrap()));
    });

    // Mixed int/float arithmetic
    group.bench_function("mixed_arithmetic", |b| {
        let mut vm = VM::new();
        let mut symbols = SymbolTable::new();
        register_primitives(&mut vm, &mut symbols);
        let value = read_str("(+ 1 2.5 3)", &mut symbols).unwrap();
        let expr = value_to_expr(&value, &mut symbols).unwrap();
        let bytecode = compile(&expr);
        b.iter(|| black_box(vm.execute(&bytecode).unwrap()));
    });

    // Comparisons
    group.bench_function("comparison", |b| {
        let mut vm = VM::new();
        let mut symbols = SymbolTable::new();
        register_primitives(&mut vm, &mut symbols);
        let value = read_str("(< 5 10)", &mut symbols).unwrap();
        let expr = value_to_expr(&value, &mut symbols).unwrap();
        let bytecode = compile(&expr);
        b.iter(|| black_box(vm.execute(&bytecode).unwrap()));
    });

    // List construction
    group.bench_function("cons", |b| {
        let mut vm = VM::new();
        let mut symbols = SymbolTable::new();
        register_primitives(&mut vm, &mut symbols);
        let value = read_str("(cons 1 (cons 2 (cons 3 nil)))", &mut symbols).unwrap();
        let expr = value_to_expr(&value, &mut symbols).unwrap();
        let bytecode = compile(&expr);
        b.iter(|| black_box(vm.execute(&bytecode).unwrap()));
    });

    // List access
    group.bench_function("first", |b| {
        let mut vm = VM::new();
        let mut symbols = SymbolTable::new();
        register_primitives(&mut vm, &mut symbols);
        let value = read_str("(first (list 1 2 3))", &mut symbols).unwrap();
        let expr = value_to_expr(&value, &mut symbols).unwrap();
        let bytecode = compile(&expr);
        b.iter(|| black_box(vm.execute(&bytecode).unwrap()));
    });

    group.finish();
}

// DEFENSE: Real-world code has conditionals
fn bench_conditionals(c: &mut Criterion) {
    let mut group = c.benchmark_group("conditionals");

    // Simple if
    group.bench_function("if_true", |b| {
        let mut vm = VM::new();
        let mut symbols = SymbolTable::new();
        register_primitives(&mut vm, &mut symbols);
        let value = read_str("(if (> 5 3) 100 200)", &mut symbols).unwrap();
        let expr = value_to_expr(&value, &mut symbols).unwrap();
        let bytecode = compile(&expr);
        b.iter(|| black_box(vm.execute(&bytecode).unwrap()));
    });

    // Nested if
    group.bench_function("nested_if", |b| {
        let mut vm = VM::new();
        let mut symbols = SymbolTable::new();
        register_primitives(&mut vm, &mut symbols);
        let value = read_str("(if (> 5 3) (if (< 2 4) 1 2) 3)", &mut symbols).unwrap();
        let expr = value_to_expr(&value, &mut symbols).unwrap();
        let bytecode = compile(&expr);
        b.iter(|| black_box(vm.execute(&bytecode).unwrap()));
    });

    group.finish();
}

// DEFENSE: End-to-end measures total pipeline overhead
fn bench_end_to_end(c: &mut Criterion) {
    let mut group = c.benchmark_group("end_to_end");

    // Simple expression
    group.bench_function("simple", |b| {
        b.iter_batched(
            || {
                let mut vm = VM::new();
                let mut symbols = SymbolTable::new();
                register_primitives(&mut vm, &mut symbols);
                (vm, symbols)
            },
            |(mut vm, mut symbols)| {
                let value = read_str("(+ 1 2 3)", &mut symbols).unwrap();
                let expr = value_to_expr(&value, &mut symbols).unwrap();
                let bytecode = compile(&expr);
                black_box(vm.execute(&bytecode).unwrap())
            },
            criterion::BatchSize::SmallInput,
        );
    });

    // Complex expression
    group.bench_function("complex", |b| {
        b.iter_batched(
            || {
                let mut vm = VM::new();
                let mut symbols = SymbolTable::new();
                register_primitives(&mut vm, &mut symbols);
                (vm, symbols)
            },
            |(mut vm, mut symbols)| {
                let value = read_str("(+ (* 2 3) (- 10 (/ 8 2)))", &mut symbols).unwrap();
                let expr = value_to_expr(&value, &mut symbols).unwrap();
                let bytecode = compile(&expr);
                black_box(vm.execute(&bytecode).unwrap())
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

// DEFENSE: Measure scalability with input size
fn bench_scalability(c: &mut Criterion) {
    let mut group = c.benchmark_group("scalability");

    for size in [10, 50, 100, 500].iter() {
        // List construction
        group.bench_with_input(
            BenchmarkId::new("list_construction", size),
            size,
            |b, &size| {
                let mut vm = VM::new();
                let mut symbols = SymbolTable::new();
                register_primitives(&mut vm, &mut symbols);

                let expr_str = format!(
                    "(list {})",
                    (0..size)
                        .map(|i| i.to_string())
                        .collect::<Vec<_>>()
                        .join(" ")
                );
                let value = read_str(&expr_str, &mut symbols).unwrap();
                let expr = value_to_expr(&value, &mut symbols).unwrap();
                let bytecode = compile(&expr);

                b.iter(|| black_box(vm.execute(&bytecode).unwrap()));
            },
        );

        // Arithmetic chain
        group.bench_with_input(
            BenchmarkId::new("addition_chain", size),
            size,
            |b, &size| {
                let mut vm = VM::new();
                let mut symbols = SymbolTable::new();
                register_primitives(&mut vm, &mut symbols);

                let expr_str = format!(
                    "(+ {})",
                    (0..size)
                        .map(|i| i.to_string())
                        .collect::<Vec<_>>()
                        .join(" ")
                );
                let value = read_str(&expr_str, &mut symbols).unwrap();
                let expr = value_to_expr(&value, &mut symbols).unwrap();
                let bytecode = compile(&expr);

                b.iter(|| black_box(vm.execute(&bytecode).unwrap()));
            },
        );
    }

    group.finish();
}

// DEFENSE: Memory operations matter for list-heavy code
fn bench_memory_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_operations");

    // Rc cloning (happens on every value copy)
    group.bench_function("value_clone", |b| {
        let mut symbols = SymbolTable::new();
        let value = read_str("(1 2 3 4 5)", &mut symbols).unwrap();
        b.iter(|| black_box(value.clone()));
    });

    // List traversal
    group.bench_function("list_to_vec", |b| {
        let mut symbols = SymbolTable::new();
        let value = read_str("(1 2 3 4 5 6 7 8 9 10)", &mut symbols).unwrap();
        b.iter(|| black_box(value.list_to_vec().unwrap()));
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_parsing,
    bench_symbol_interning,
    bench_compilation,
    bench_vm_execution,
    bench_conditionals,
    bench_end_to_end,
    bench_scalability,
    bench_memory_operations,
);

criterion_main!(benches);
