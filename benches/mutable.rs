// audited: 2026-10-23
//! The sorted-store gate: an `@set` and a `@struct` beside std's B-trees at 10³, 10⁴ and 10⁵ random keys.
//!
//! docs/impl/mutable.md
//!
//! Run with: cargo bench --bench mutable
//!
//! Each row inserts every key and then deletes every key, once through the
//! store funnels and once on a `BTreeSet` or a `BTreeMap` of the same keys.
//! It reports the fastest of five runs of each, and the larger of the two
//! ratios of funnel time to std time. A ratio above the bound at the gated
//! size fails the run with exit status 1.

use std::collections::{BTreeMap, BTreeSet};
use std::hint::black_box;
use std::time::{Duration, Instant};

use elle::primitives::ctx::TestHeap;
use elle::value::arena::{
    set_add_with_incref, set_del_with_decref, struct_put_with_rebind, struct_remove_with_decref,
};
use elle::value::types::TableKey;
use elle::Value;

const SIZES: [usize; 3] = [1_000, 10_000, 100_000];
const GATED: usize = 100_000;
const BOUND: f64 = 2.0;
const RUNS: usize = 5;

/// The time to insert every key, and the time to delete every key after.
type Phases = (Duration, Duration);

/// `n` distinct keys in a scattered order: 7919 is prime to the modulus.
fn keys(n: usize) -> Vec<i64> {
    (0..n as i64).map(|i| i * 7919 % 1_000_003).collect()
}

/// The fastest insert and the fastest delete over `RUNS` runs of `run`.
fn best(run: impl Fn() -> Phases) -> Phases {
    (0..RUNS)
        .map(|_| run())
        .reduce(|a, b| (a.0.min(b.0), a.1.min(b.1)))
        .expect("at least one run")
}

/// Time `insert` over every key on `state`, then `delete` over every key.
fn phases<S>(
    keys: &[i64],
    state: &mut S,
    insert: impl Fn(&mut S, i64),
    delete: impl Fn(&mut S, i64),
) -> Phases {
    let start = Instant::now();
    for &k in keys {
        insert(state, k);
    }
    let inserted = start.elapsed();
    let start = Instant::now();
    for &k in keys {
        delete(state, k);
    }
    (inserted, start.elapsed())
}

fn set_funnels(keys: &[i64]) -> Phases {
    let h = TestHeap::new();
    let mut set = h.ctx().set_mut(BTreeSet::new());
    phases(
        keys,
        &mut set,
        |set, k| {
            black_box(set_add_with_incref(h.heap(), *set, Value::int(k)));
        },
        |set, k| {
            black_box(set_del_with_decref(h.heap(), *set, &Value::int(k)));
        },
    )
}

fn set_std(keys: &[i64]) -> Phases {
    phases(
        keys,
        &mut BTreeSet::new(),
        |set, k| {
            black_box(set.insert(Value::int(k)));
        },
        |set, k| {
            black_box(set.remove(&Value::int(k)));
        },
    )
}

fn struct_funnels(keys: &[i64]) -> Phases {
    let h = TestHeap::new();
    let mut st = h.ctx().struct_mut();
    phases(
        keys,
        &mut st,
        |st, k| {
            black_box(struct_put_with_rebind(
                h.heap(),
                *st,
                TableKey::Int(k),
                Value::int(k),
            ));
        },
        |st, k| {
            black_box(struct_remove_with_decref(h.heap(), *st, &TableKey::Int(k)));
        },
    )
}

fn struct_std(keys: &[i64]) -> Phases {
    phases(
        keys,
        &mut BTreeMap::new(),
        |map, k| {
            black_box(map.insert(TableKey::Int(k), Value::int(k)));
        },
        |map, k| {
            black_box(map.remove(&TableKey::Int(k)));
        },
    )
}

fn ratio(ours: Duration, std: Duration) -> f64 {
    ours.as_secs_f64() / std.as_secs_f64().max(f64::MIN_POSITIVE)
}

/// Print one row, and answer whether it stays within the bound.
fn row(name: &str, n: usize, ours: Phases, std: Phases) -> bool {
    let worst = ratio(ours.0, std.0).max(ratio(ours.1, std.1));
    println!(
        "{name:<8} {n:>7} {:>10.5} {:>10.5} {:>10.5} {:>10.5} {worst:>7.2}",
        ours.0.as_secs_f64(),
        ours.1.as_secs_f64(),
        std.0.as_secs_f64(),
        std.1.as_secs_f64(),
    );
    n != GATED || worst <= BOUND
}

fn main() {
    println!(
        "{:<8} {:>7} {:>10} {:>10} {:>10} {:>10} {:>7}",
        "store", "keys", "insert s", "delete s", "std ins s", "std del s", "ratio"
    );
    let mut within = true;
    for n in SIZES {
        let keys = keys(n);
        let set = row(
            "@set",
            n,
            best(|| set_funnels(&keys)),
            best(|| set_std(&keys)),
        );
        let st = row(
            "@struct",
            n,
            best(|| struct_funnels(&keys)),
            best(|| struct_std(&keys)),
        );
        within &= set && st;
    }
    if within {
        println!("every store stays within {BOUND}x of std at {GATED} keys");
    } else {
        println!("a store exceeds {BOUND}x of std at {GATED} keys");
        std::process::exit(1);
    }
}
