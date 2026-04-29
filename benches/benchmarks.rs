use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use elle::pipeline::{compile, eval, eval_all};
use elle::primitives::register_primitives;
use elle::{read_str, SymbolTable, VM};

fn setup() -> (VM, SymbolTable) {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    let _signals = register_primitives(&mut vm, &mut symbols);
    (vm, symbols)
}

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

    // Simple arithmetic
    group.bench_function("simple_arithmetic", |b| {
        b.iter_batched(
            || {
                let (_, symbols) = setup();
                symbols
            },
            |mut symbols| black_box(compile("(+ 1 2)", &mut symbols, "<benchmark>").unwrap()),
            criterion::BatchSize::SmallInput,
        );
    });

    // Conditional
    group.bench_function("conditional", |b| {
        b.iter_batched(
            || {
                let (_, symbols) = setup();
                symbols
            },
            |mut symbols| {
                black_box(compile("(if (> 5 3) 100 200)", &mut symbols, "<benchmark>").unwrap())
            },
            criterion::BatchSize::SmallInput,
        );
    });

    // Nested expressions
    group.bench_function("nested_arithmetic", |b| {
        b.iter_batched(
            || {
                let (_, symbols) = setup();
                symbols
            },
            |mut symbols| {
                black_box(
                    compile("(+ (* 2 3) (- 10 (/ 8 2)))", &mut symbols, "<benchmark>").unwrap(),
                )
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

// DEFENSE: VM execution is the hot path
fn bench_vm_execution(c: &mut Criterion) {
    let mut group = c.benchmark_group("vm_execution");

    // Integer arithmetic (specialized ops)
    group.bench_function("int_add", |b| {
        let (mut vm, mut symbols) = setup();
        let result = compile("(+ 1 2 3 4 5)", &mut symbols, "<benchmark>").unwrap();
        b.iter(|| black_box(vm.execute(&result.bytecode).unwrap()));
    });

    // Mixed int/float arithmetic
    group.bench_function("mixed_arithmetic", |b| {
        let (mut vm, mut symbols) = setup();
        let result = compile("(+ 1 2.5 3)", &mut symbols, "<benchmark>").unwrap();
        b.iter(|| black_box(vm.execute(&result.bytecode).unwrap()));
    });

    // Comparisons
    group.bench_function("comparison", |b| {
        let (mut vm, mut symbols) = setup();
        let result = compile("(< 5 10)", &mut symbols, "<benchmark>").unwrap();
        b.iter(|| black_box(vm.execute(&result.bytecode).unwrap()));
    });

    // List construction
    group.bench_function("pair", |b| {
        let (mut vm, mut symbols) = setup();
        let result = compile(
            "(pair 1 (pair 2 (pair 3 nil)))",
            &mut symbols,
            "<benchmark>",
        )
        .unwrap();
        b.iter(|| black_box(vm.execute(&result.bytecode).unwrap()));
    });

    // List access
    group.bench_function("first", |b| {
        let (mut vm, mut symbols) = setup();
        let result = compile("(first (list 1 2 3))", &mut symbols, "<benchmark>").unwrap();
        b.iter(|| black_box(vm.execute(&result.bytecode).unwrap()));
    });

    group.finish();
}

// DEFENSE: Real-world code has conditionals
fn bench_conditionals(c: &mut Criterion) {
    let mut group = c.benchmark_group("conditionals");

    // Simple if
    group.bench_function("if_true", |b| {
        let (mut vm, mut symbols) = setup();
        let result = compile("(if (> 5 3) 100 200)", &mut symbols, "<benchmark>").unwrap();
        b.iter(|| black_box(vm.execute(&result.bytecode).unwrap()));
    });

    // Nested if
    group.bench_function("nested_if", |b| {
        let (mut vm, mut symbols) = setup();
        let result = compile(
            "(if (> 5 3) (if (< 2 4) 1 2) 3)",
            &mut symbols,
            "<benchmark>",
        )
        .unwrap();
        b.iter(|| black_box(vm.execute(&result.bytecode).unwrap()));
    });

    group.finish();
}

// DEFENSE: End-to-end measures total pipeline overhead
fn bench_end_to_end(c: &mut Criterion) {
    let mut group = c.benchmark_group("end_to_end");

    // Simple expression
    group.bench_function("simple", |b| {
        b.iter_batched(
            setup,
            |(mut vm, mut symbols)| {
                black_box(eval("(+ 1 2 3)", &mut symbols, &mut vm, "<benchmark>").unwrap())
            },
            criterion::BatchSize::SmallInput,
        );
    });

    // Complex expression
    group.bench_function("complex", |b| {
        b.iter_batched(
            setup,
            |(mut vm, mut symbols)| {
                black_box(
                    eval(
                        "(+ (* 2 3) (- 10 (/ 8 2)))",
                        &mut symbols,
                        &mut vm,
                        "<benchmark>",
                    )
                    .unwrap(),
                )
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
                let (mut vm, mut symbols) = setup();

                let expr_str = format!(
                    "(list {})",
                    (0..size)
                        .map(|i| i.to_string())
                        .collect::<Vec<_>>()
                        .join(" ")
                );
                let result = compile(&expr_str, &mut symbols, "<benchmark>").unwrap();

                b.iter(|| black_box(vm.execute(&result.bytecode).unwrap()));
            },
        );

        // Arithmetic chain
        group.bench_with_input(
            BenchmarkId::new("addition_chain", size),
            size,
            |b, &size| {
                let (mut vm, mut symbols) = setup();

                let expr_str = format!(
                    "(+ {})",
                    (0..size)
                        .map(|i| i.to_string())
                        .collect::<Vec<_>>()
                        .join(" ")
                );
                let result = compile(&expr_str, &mut symbols, "<benchmark>").unwrap();

                b.iter(|| black_box(vm.execute(&result.bytecode).unwrap()));
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
        b.iter(|| black_box(value));
    });

    // List traversal
    group.bench_function("list_to_vec", |b| {
        let mut symbols = SymbolTable::new();
        let value = read_str("(1 2 3 4 5 6 7 8 9 10)", &mut symbols).unwrap();
        b.iter(|| black_box(value.list_to_vec().unwrap()));
    });

    group.finish();
}

// Fresh VM + SymbolTable with primitives registered, no prelude loaded.
// Used by bench_macro_expansion so prelude loading is included in each iteration.
fn setup_vm() -> (VM, SymbolTable) {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    register_primitives(&mut vm, &mut symbols);
    (vm, symbols)
}

// DEFENSE: Macro expansion throughput — measures the full pipeline cost
// (including prelude loading) for macro-heavy Elle snippets.
//
// Each iteration uses a fresh VM so the transformer cache starts cold,
// matching the cost a user pays per compilation unit. The caching benefit
// (issue #562) shows up within a single iteration when the same macro is
// invoked many times: the first call compiles the transformer closure;
// subsequent calls reuse it via VM::call_closure.
fn bench_macro_expansion(c: &mut Criterion) {
    let mut group = c.benchmark_group("macro_expansion");

    // 100 `when` invocations — prelude macro, used extensively.
    // After the first expansion, the `when` transformer closure is cached;
    // invocations 2–100 call it directly without recompiling.
    group.bench_function("when_100", |b| {
        let source = (0..100)
            .map(|i| format!("(when true {})", i))
            .collect::<Vec<_>>()
            .join("\n");
        b.iter_batched(
            setup_vm,
            |(mut vm, mut symbols)| {
                black_box(eval_all(&source, &mut symbols, &mut vm, "<bench>").unwrap())
            },
            criterion::BatchSize::SmallInput,
        );
    });

    // Thread-first macro — 9 chained applications.
    // `->` is a recursive prelude macro; the transformer closure is compiled
    // once and reused for each application in the chain.
    group.bench_function("thread_first_9", |b| {
        let source = "(-> 1 (+ 2) (+ 3) (+ 4) (+ 5) (+ 6) (+ 7) (+ 8) (+ 9) (+ 10))";
        b.iter_batched(
            setup_vm,
            |(mut vm, mut symbols)| {
                black_box(eval_all(source, &mut symbols, &mut vm, "<bench>").unwrap())
            },
            criterion::BatchSize::SmallInput,
        );
    });

    // `defn` — the most commonly used prelude macro.
    // 50 function definitions, each expanding `defn` to `(def name (fn ...))`.
    group.bench_function("defn_50", |b| {
        let source = (0..50)
            .map(|i| format!("(defn f{} (x) (+ x {}))", i, i))
            .collect::<Vec<_>>()
            .join("\n");
        b.iter_batched(
            setup_vm,
            |(mut vm, mut symbols)| {
                black_box(eval_all(&source, &mut symbols, &mut vm, "<bench>").unwrap())
            },
            criterion::BatchSize::SmallInput,
        );
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
    bench_macro_expansion,
);

criterion_main!(benches);
