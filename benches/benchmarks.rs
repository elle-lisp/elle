use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use elle::pipeline::{compile, eval, eval_all};
use elle::runtime::RuntimeCore;
use elle::value::FiberHeap;
use elle::{read_str, SymbolTable};

// A bare Elle instance for the benches: primitives registered, a fresh
// `CompileCtx` (core.lisp + prelude), and the VM wired to its compile context —
// but no stdlib. The compile pipeline takes the compile context explicitly as a
// parameter, so every `compile`/`eval`/`eval_all` call below threads
// `core.parts()`.
fn setup() -> RuntimeCore {
    RuntimeCore::bare()
}

// DEFENSE: Separate parsing from execution to measure each phase independently
fn bench_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("parsing");
    let mut symbols = SymbolTable::new();
    // `read_str` is born in a fresh region on the caller's heap (the read result
    // escapes value-based), so the parse benches need an instance heap to allocate
    // into. The benchmark process is short-lived; the read residue is irrelevant.
    let mut heap = FiberHeap::new();

    // Simple expression
    group.bench_function("simple_number", |b| {
        b.iter(|| black_box(read_str("42", &mut heap, &mut symbols).unwrap()));
    });

    // List with numbers
    group.bench_function("list_literal", |b| {
        b.iter(|| black_box(read_str("(1 2 3 4 5)", &mut heap, &mut symbols).unwrap()));
    });

    // Nested expression
    group.bench_function("nested_expr", |b| {
        b.iter(|| black_box(read_str("(+ (* 2 3) (- 10 5))", &mut heap, &mut symbols).unwrap()));
    });

    // Deep nesting
    group.bench_function("deep_nesting", |b| {
        b.iter(|| black_box(read_str("(((((1)))))", &mut heap, &mut symbols).unwrap()));
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
        b.iter(|| black_box(read_str(&large_list, &mut heap, &mut symbols).unwrap()));
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
            setup,
            |mut core| {
                let (_, symbols, cctx) = core.parts();
                black_box(compile("(+ 1 2)", symbols, cctx, "<benchmark>").unwrap())
            },
            criterion::BatchSize::SmallInput,
        );
    });

    // Conditional
    group.bench_function("conditional", |b| {
        b.iter_batched(
            setup,
            |mut core| {
                let (_, symbols, cctx) = core.parts();
                black_box(compile("(if (> 5 3) 100 200)", symbols, cctx, "<benchmark>").unwrap())
            },
            criterion::BatchSize::SmallInput,
        );
    });

    // Nested expressions
    group.bench_function("nested_arithmetic", |b| {
        b.iter_batched(
            setup,
            |mut core| {
                let (_, symbols, cctx) = core.parts();
                black_box(
                    compile("(+ (* 2 3) (- 10 (/ 8 2)))", symbols, cctx, "<benchmark>").unwrap(),
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
        let mut core = setup();
        let (vm, symbols, cctx) = core.parts();
        let result = compile("(+ 1 2 3 4 5)", symbols, cctx, "<benchmark>").unwrap();
        b.iter(|| black_box(vm.execute(&result.bytecode).unwrap()));
    });

    // Mixed int/float arithmetic
    group.bench_function("mixed_arithmetic", |b| {
        let mut core = setup();
        let (vm, symbols, cctx) = core.parts();
        let result = compile("(+ 1 2.5 3)", symbols, cctx, "<benchmark>").unwrap();
        b.iter(|| black_box(vm.execute(&result.bytecode).unwrap()));
    });

    // Comparisons
    group.bench_function("comparison", |b| {
        let mut core = setup();
        let (vm, symbols, cctx) = core.parts();
        let result = compile("(< 5 10)", symbols, cctx, "<benchmark>").unwrap();
        b.iter(|| black_box(vm.execute(&result.bytecode).unwrap()));
    });

    // List construction
    group.bench_function("pair", |b| {
        let mut core = setup();
        let (vm, symbols, cctx) = core.parts();
        let result = compile(
            "(pair 1 (pair 2 (pair 3 nil)))",
            symbols,
            cctx,
            "<benchmark>",
        )
        .unwrap();
        b.iter(|| black_box(vm.execute(&result.bytecode).unwrap()));
    });

    // List access
    group.bench_function("first", |b| {
        let mut core = setup();
        let (vm, symbols, cctx) = core.parts();
        let result = compile("(first (list 1 2 3))", symbols, cctx, "<benchmark>").unwrap();
        b.iter(|| black_box(vm.execute(&result.bytecode).unwrap()));
    });

    group.finish();
}

// DEFENSE: Real-world code has conditionals
fn bench_conditionals(c: &mut Criterion) {
    let mut group = c.benchmark_group("conditionals");

    // Simple if
    group.bench_function("if_true", |b| {
        let mut core = setup();
        let (vm, symbols, cctx) = core.parts();
        let result = compile("(if (> 5 3) 100 200)", symbols, cctx, "<benchmark>").unwrap();
        b.iter(|| black_box(vm.execute(&result.bytecode).unwrap()));
    });

    // Nested if
    group.bench_function("nested_if", |b| {
        let mut core = setup();
        let (vm, symbols, cctx) = core.parts();
        let result = compile(
            "(if (> 5 3) (if (< 2 4) 1 2) 3)",
            symbols,
            cctx,
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
            |mut core| {
                let (vm, symbols, cctx) = core.parts();
                black_box(eval("(+ 1 2 3)", symbols, vm, cctx, "<benchmark>").unwrap())
            },
            criterion::BatchSize::SmallInput,
        );
    });

    // Complex expression
    group.bench_function("complex", |b| {
        b.iter_batched(
            setup,
            |mut core| {
                let (vm, symbols, cctx) = core.parts();
                black_box(
                    eval(
                        "(+ (* 2 3) (- 10 (/ 8 2)))",
                        symbols,
                        vm,
                        cctx,
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
                let mut core = setup();
                let (vm, symbols, cctx) = core.parts();

                let expr_str = format!(
                    "(list {})",
                    (0..size)
                        .map(|i| i.to_string())
                        .collect::<Vec<_>>()
                        .join(" ")
                );
                let result = compile(&expr_str, symbols, cctx, "<benchmark>").unwrap();

                b.iter(|| black_box(vm.execute(&result.bytecode).unwrap()));
            },
        );

        // Arithmetic chain
        group.bench_with_input(
            BenchmarkId::new("addition_chain", size),
            size,
            |b, &size| {
                let mut core = setup();
                let (vm, symbols, cctx) = core.parts();

                let expr_str = format!(
                    "(+ {})",
                    (0..size)
                        .map(|i| i.to_string())
                        .collect::<Vec<_>>()
                        .join(" ")
                );
                let result = compile(&expr_str, symbols, cctx, "<benchmark>").unwrap();

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
        let mut heap = FiberHeap::new();
        let value = read_str("(1 2 3 4 5)", &mut heap, &mut symbols).unwrap();
        b.iter(|| black_box(value));
    });

    // List traversal
    group.bench_function("list_to_vec", |b| {
        let mut symbols = SymbolTable::new();
        let mut heap = FiberHeap::new();
        let value = read_str("(1 2 3 4 5 6 7 8 9 10)", &mut heap, &mut symbols).unwrap();
        b.iter(|| black_box(value.list_to_vec().unwrap()));
    });

    group.finish();
}

// DEFENSE: Macro expansion throughput — measures the full pipeline cost
// (expand → analyze → lower → emit → execute) for macro-heavy Elle snippets.
//
// Each batch gets a fresh instance (`setup` → `RuntimeCore::bare`), so the
// per-compile transformer cache starts cold — matching the cost a user pays per
// compilation unit. (core.lisp + the prelude are loaded once when the instance
// is built, in the unmeasured batch setup; under the explicit-`CompileCtx`
// design each eval threads its own compile context, so there is nothing to
// reload per eval.) The caching
// benefit (issue #562) shows up within a single `eval_all` when the same macro
// is invoked many times: the first call compiles the transformer closure;
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
            setup,
            |mut core| {
                let (vm, symbols, cctx) = core.parts();
                black_box(eval_all(&source, symbols, vm, cctx, "<bench>").unwrap())
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
            setup,
            |mut core| {
                let (vm, symbols, cctx) = core.parts();
                black_box(eval_all(source, symbols, vm, cctx, "<bench>").unwrap())
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
            setup,
            |mut core| {
                let (vm, symbols, cctx) = core.parts();
                black_box(eval_all(&source, symbols, vm, cctx, "<bench>").unwrap())
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
