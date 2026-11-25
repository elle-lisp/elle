// audited: 2026-11-25
//! A warm interpreted call allocates nothing on the Rust heap, whatever its callee.
//!
//! docs/impl/spares.md
//!
//! Each test runs a loop of calls in the interpreter twice, once short and once
//! long, and reads what the extra calls allocated. The run's own setup costs
//! the same at both lengths, so the difference is the calls' alone.

use std::collections::BTreeMap;

use crate::config::JitPolicy;
use crate::pipeline::compile_file_repl;
use crate::runtime::Runtime;
use crate::value::Value;

// The counter-factual is a call that gathers its arguments into a fresh Vec,
// moves its paused caller's stack into one, or copies its callee's environment
// into a fresh `Rc`: each costs an allocation a call, so the extra calls read
// at least `EXTRA`.

/// The program's answer, an array of what the tests call:
///
/// 0. a driver that calls its first argument on `k` and `k`, `n` times, in
///    call position;
/// 1. a closure of two parameters, which answers its first;
/// 2. a function that tail-calls itself by name `n` times;
/// 3. the native `bit/and`;
/// 4. a driver that calls its first argument `n` times on the elements of its
///    second, spliced;
/// 5. the array `[1 2]`.
const PROGRAM: &str = "(defn spin [n] (if (= n 0) 0 (spin (- n 1)))) \
                       [(fn [g n] \
                          (var k 0) \
                          (while (< k n) (g k k) (assign k (inc k))) \
                          k) \
                        (fn [a b] a) \
                        spin \
                        bit/and \
                        (fn [g xs n] \
                          (var k 0) \
                          (while (< k n) (g ;xs) (assign k (inc k))) \
                          k) \
                        [1 2]]";

/// How many more calls the long run makes than the short one.
const EXTRA: i64 = 100;

/// A run of calls: the function to run, and its arguments.
type Run = (Value, Vec<Value>);

struct Fixture {
    rt: Runtime,
    values: Vec<Value>,
}

impl Fixture {
    /// The program's values on a runtime whose JIT is off, so every call runs
    /// in the interpreter.
    fn new() -> Self {
        let mut rt = Runtime::new();
        rt.vm().runtime_config.jit = JitPolicy::Off;
        let prog = {
            let (_vm, symbols, cctx) = rt.parts();
            compile_file_repl(PROGRAM, symbols, cctx, "<call-allocs>")
                .expect("compiles")
                .0
        };
        let program = {
            let (vm, _symbols, cctx) = rt.parts();
            vm.execute_scheduled(&prog, cctx).expect("runs")
        };
        let values = program
            .array_to_vec()
            .expect("the program answers an array");
        Fixture { rt, values }
    }

    /// Run `run` in the interpreter, and answer the allocations it made.
    fn allocations(&mut self, run: &Run) -> u64 {
        let vm = self.rt.vm();
        let (answer, made) = crate::testalloc::allocations(|| vm.call_closure(run.0, &run.1));
        answer.expect("the run answers");
        made
    }

    /// Run `run` in the interpreter, and answer the place each of its
    /// allocations came from, with how many it made there.
    fn sites(&mut self, run: &Run) -> BTreeMap<String, u64> {
        let vm = self.rt.vm();
        let (answer, sites) = crate::testalloc::sites(|| vm.call_closure(run.0, &run.1));
        answer.expect("the run answers");
        let mut counts = BTreeMap::new();
        for site in sites {
            *counts.entry(site).or_insert(0) += 1;
        }
        counts
    }

    /// What `EXTRA` more calls allocate, where `runs(n)` makes `n` calls: the
    /// long run warms the VM first. Fails naming each place the extra calls
    /// allocated, with how many times.
    fn assert_calls_allocate_nothing(&mut self, what: &str, runs: impl Fn(&Fixture, i64) -> Run) {
        let long = runs(self, 10 + EXTRA);
        let short = runs(self, 10);
        self.allocations(&long);
        let at_short = self.allocations(&short);
        let at_long = self.allocations(&long);
        if at_long <= at_short {
            return;
        }
        let mut sites = self.sites(&long);
        for (site, n) in self.sites(&short) {
            let left = sites.entry(site).or_insert(0);
            *left = left.saturating_sub(n);
        }
        sites.retain(|_, n| *n > 0);
        panic!(
            "{EXTRA} {what} allocated {} times, at {sites:#?}",
            at_long - at_short
        );
    }
}

/// The driver's run of `n` calls to the value at `callee`.
fn driving(callee: usize) -> impl Fn(&Fixture, i64) -> Run {
    move |f, n| (f.values[0], vec![f.values[callee], Value::int(n)])
}

#[test]
fn a_closure_call_allocates_nothing() {
    Fixture::new().assert_calls_allocate_nothing("calls to a closure", driving(1));
}

#[test]
fn a_native_call_allocates_nothing() {
    Fixture::new().assert_calls_allocate_nothing("calls to a native", driving(3));
}

#[test]
fn a_tail_call_allocates_nothing() {
    Fixture::new().assert_calls_allocate_nothing("tail calls to a closure", |f, n| {
        (f.values[2], vec![Value::int(n)])
    });
}

#[test]
fn a_spliced_call_allocates_nothing() {
    Fixture::new().assert_calls_allocate_nothing("spliced calls to a closure", |f, n| {
        (f.values[4], vec![f.values[1], f.values[5], Value::int(n)])
    });
}
