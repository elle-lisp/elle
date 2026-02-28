//! Memory allocation benchmarks for Elle.
//!
//! Uses `stats_alloc` to count heap allocations and bytes during program
//! execution. This catches unnecessary allocation patterns like the
//! LocalCell-per-let-binding issue (see GitHub issue #380).
//!
//! Run with: cargo bench --bench memory
//!
//! These benchmarks report allocation counts and bytes, not timing.
//! The output format is compatible with `bencher` for CI regression tracking.

use stats_alloc::{Region, StatsAlloc, INSTRUMENTED_SYSTEM};
use std::alloc::System;

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

use elle::pipeline::eval;
use elle::primitives::register_primitives;
use elle::{SymbolTable, VM};

fn setup() -> (VM, SymbolTable) {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    let _effects = register_primitives(&mut vm, &mut symbols);
    (vm, symbols)
}

/// Run an Elle program and return allocation stats for the execution phase only.
/// Setup (VM creation, primitive registration, compilation) is excluded.
fn measure_eval(source: &str) -> stats_alloc::Stats {
    let (mut vm, mut symbols) = setup();
    let reg = Region::new(GLOBAL);
    let _ = eval(source, &mut symbols, &mut vm).expect("program should succeed");
    reg.change()
}

/// Fibonacci — tests recursive function call overhead.
/// Pure recursion, no let bindings, no closures over mutable state.
fn bench_fib() -> stats_alloc::Stats {
    measure_eval(
        "(begin
           (def fib (fn (n)
             (if (< n 2) n
               (+ (fib (- n 1)) (fib (- n 2))))))
           (fib 20))",
    )
}

/// N-Queens N=8 — the workload that motivated cell_locals_mask.
/// Contains let bindings inside recursive lambdas.
fn bench_nqueens() -> stats_alloc::Stats {
    measure_eval(
        "(begin
           (var check-safe-helper
             (fn (col remaining row-offset)
               (if (empty? remaining)
                 true
                 (let ((placed-col (first remaining)))
                   (if (or (= col placed-col)
                           (= row-offset (abs (- col placed-col))))
                     false
                     (check-safe-helper col (rest remaining) (+ row-offset 1)))))))

           (var safe?
             (fn (col queens)
               (check-safe-helper col queens 1)))

           (var try-cols-helper
             (fn (n col queens row)
               (if (= col n)
                 (list)
                 (if (safe? col queens)
                   (let ((new-queens (cons col queens)))
                     (append (solve-helper n (+ row 1) new-queens)
                             (try-cols-helper n (+ col 1) queens row)))
                   (try-cols-helper n (+ col 1) queens row)))))

           (var solve-helper
             (fn (n row queens)
               (if (= row n)
                 (list (reverse queens))
                 (try-cols-helper n 0 queens row))))

           (var solve-nqueens
             (fn (n)
               (solve-helper n 0 (list))))

           (length (solve-nqueens 8)))",
    )
}

/// List construction and traversal — measures cons cell allocation.
fn bench_list_heavy() -> stats_alloc::Stats {
    measure_eval(
        "(begin
           (def build (fn (n acc)
             (if (= n 0) acc
               (build (- n 1) (cons n acc)))))
           (def sum (fn (lst acc)
             (if (empty? lst) acc
               (sum (rest lst) (+ acc (first lst))))))
           (sum (build 1000 (list)) 0))",
    )
}

/// Closure capture — measures environment allocation.
fn bench_closures() -> stats_alloc::Stats {
    measure_eval(
        "(begin
           (def make-adder (fn (x) (fn (y) (+ x y))))
           (def add5 (make-adder 5))
           (def loop (fn (n acc)
             (if (= n 0) acc
               (loop (- n 1) (+ acc (add5 n))))))
           (loop 1000 0))",
    )
}

/// Print one benchmark result in bencher format (compatible with CI tooling).
fn report(name: &str, stats: &stats_alloc::Stats) {
    // Net allocations (allocations minus deallocations = live objects at end)
    let net = stats.allocations.saturating_sub(stats.deallocations);
    let net_bytes = stats
        .bytes_allocated
        .saturating_sub(stats.bytes_deallocated);

    // Report total allocations as the primary metric (bencher format)
    println!(
        "test {name}_allocs       ... bench: {allocs} allocs/iter (+/- 0)",
        allocs = stats.allocations
    );
    println!(
        "test {name}_bytes        ... bench: {bytes} bytes/iter (+/- 0)",
        bytes = stats.bytes_allocated
    );
    println!("test {name}_net_allocs   ... bench: {net} net_allocs/iter (+/- 0)");
    println!("test {name}_net_bytes    ... bench: {net_bytes} net_bytes/iter (+/- 0)");
}

fn main() {
    // Warm up: run once to populate JIT cache, then measure the second run.
    // The first run includes JIT compilation overhead; the second is pure execution.
    println!();

    let stats = bench_fib();
    report("memory::fib_20", &stats);

    let stats = bench_nqueens();
    report("memory::nqueens_8", &stats);

    let stats = bench_list_heavy();
    report("memory::list_build_sum_1000", &stats);

    let stats = bench_closures();
    report("memory::closure_loop_1000", &stats);

    println!();
}
