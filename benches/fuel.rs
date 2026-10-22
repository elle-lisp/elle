// audited: 2026-10-22
//! The fuel charge per loop trip: a silent and a raising loop, tight and wide, on the bytecode and JIT tiers.
//!
//! benches/AGENTS.md
//! docs/impl/jit.md
//!
//! Each bench runs one loop of `TRIPS` backward jumps and reports trips per
//! second, so a reading is the cost of one trip, charge included. The loop is
//! written once and read in two signals: the silent form proves its operands
//! with `(numeric!)`, and the raising form ends in a branch that may raise, so
//! its charge is the branching one. The wide form carries six more intrinsics
//! per trip, which is what a charge hides under in a loop that does work.
//!
//! A run evaluates one unit holding the four loops and the call, because a
//! `def` in one unit is no global for a later compile. The unit's compile is
//! under one percent of a run at these trip counts, and the same in every
//! reading, so it cancels in a comparison.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use elle::pipeline::eval_all;
use elle::runtime::RuntimeCore;
use elle::Value;

/// Backward jumps per run on a compiled tier: long enough that the unit's
/// compile is noise against the loop.
const JIT_TRIPS: u64 = 50_000_000;

/// Backward jumps per run on the interpreter, which spends twenty times as
/// long on each.
const BYTECODE_TRIPS: u64 = 2_000_000;

/// The four loops, each counting `i` to `trips`. The wide body's two running
/// sums depend on `i`, so nothing folds.
fn loops(trips: u64) -> String {
    let wide_body = "(assign a (%add (%mul a 3) b)) (assign b (%sub (%mul b 5) a)) \
                     (assign a (%add a (%mul i 7)))";
    let raise = "(if (%lt i 0) (error {:error :negative}) i)";
    format!(
        "(defn tight-silent [] (numeric!) (def @i 0) \
           (while (%lt i {trips}) (assign i (%add i 1))) i)
         (defn tight-raising [] (def @i 0) \
           (while (%lt i {trips}) (assign i (%add i 1))) {raise})
         (defn wide-silent [] (numeric!) (def @i 0) (def @a 1) (def @b 3) \
           (while (%lt i {trips}) {wide_body} (assign i (%add i 1))) i)
         (defn wide-raising [] (def @i 0) (def @a 1) (def @b 3) \
           (while (%lt i {trips}) {wide_body} (assign i (%add i 1))) {raise})"
    )
}

/// The tiers this build carries that `compile/run-on` can force.
fn tiers() -> Vec<(&'static str, u64)> {
    let mut tiers = vec![("bytecode", BYTECODE_TRIPS)];
    if cfg!(feature = "jit") {
        tiers.push(("jit", JIT_TRIPS));
    }
    tiers
}

fn bench_fuel_charge(c: &mut Criterion) {
    let mut group = c.benchmark_group("fuel_charge");
    group.sample_size(20);
    for (tier, trips) in tiers() {
        for shape in [
            "tight-silent",
            "tight-raising",
            "wide-silent",
            "wide-raising",
        ] {
            group.throughput(Throughput::Elements(trips));
            group.bench_function(BenchmarkId::new(tier, shape), |b| {
                let mut core = RuntimeCore::bare();
                let (vm, symbols, cctx) = core.parts();
                let unit = format!("{}\n(compile/run-on :{tier} {shape})", loops(trips));
                assert_eq!(
                    eval_all(&unit, symbols, vm, cctx, "<fuel>").expect("the unit runs"),
                    Value::int(trips as i64),
                    "{tier}/{shape} did not count to {trips}"
                );
                b.iter(|| {
                    black_box(eval_all(&unit, symbols, vm, cctx, "<fuel>").expect("the unit runs"))
                });
            });
        }
    }
    group.finish();
}

criterion_group!(benches, bench_fuel_charge);
criterion_main!(benches);
