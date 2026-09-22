// audited: 2026-09-22
//! Region-native LIR parity — the head-to-head the expander migration ran for
//! syntax, over the LIR the stdlib compile produces.
//!
//! docs/impl/image/measurements.md
//!
//! Reports build, walk, rewrite and teardown for the shipped Rust-heap
//! `LirFunction` and for a region-native prototype carrying the same graph,
//! plus the allocator traffic and the resident bytes of each. Like
//! `benches/regionrc.rs` this is a *reporting* bench: it prints numbers and
//! asserts nothing, beyond the census check that the two forms agree.
//!
//! Run with: cargo bench --bench lirshape

mod build;
mod node;
mod opcode;
mod ops;
mod size;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use elle::lir::LirFunction;
use elle::pipeline::{compile_file_to_lir, sources};
use elle::runtime::Runtime;

use build::Builder;
use node::PFunc;

/// Rounds per operation, as the expander prototype ran.
const ROUNDS: usize = 30;

// ── the allocator counter ─────────────────────────────────────────

static ALLOCS: AtomicU64 = AtomicU64::new(0);
static ALLOC_BYTES: AtomicU64 = AtomicU64::new(0);
static FREED_BYTES: AtomicU64 = AtomicU64::new(0);

/// The system allocator, counting the calls through it. A build's malloc
/// traffic is half the question this bench answers, and nothing else reports
/// it.
struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        ALLOC_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        FREED_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        System.dealloc(ptr, layout)
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        ALLOC_BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
        FREED_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        System.realloc(ptr, layout, new_size)
    }
}

/// Bytes this process holds right now: what it asked for, less what it gave
/// back.
fn live_bytes() -> u64 {
    ALLOC_BYTES.load(Ordering::Relaxed) - FREED_BYTES.load(Ordering::Relaxed)
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn allocs() -> (u64, u64) {
    (
        ALLOCS.load(Ordering::Relaxed),
        ALLOC_BYTES.load(Ordering::Relaxed),
    )
}

// ── the corpus ────────────────────────────────────────────────────

/// Every function the three boot sources lower to: the largest real Elle
/// compilation unit, and the one a boot image would have to carry.
fn corpus(rt: &mut Runtime) -> Vec<LirFunction> {
    let mut out = Vec::new();
    for (name, src) in [
        ("core.lisp", sources::CORE),
        ("prelude.lisp", sources::PRELUDE),
        ("stdlib.lisp", sources::STDLIB),
    ] {
        let (_, symbols, cctx) = rt.parts();
        match compile_file_to_lir(src, symbols, cctx, name, 0) {
            Ok(module) => {
                out.push(module.entry);
                out.extend(module.closures);
            }
            Err(e) => println!("  (skipped {name}: {e})"),
        }
    }
    out
}

// ── the measurements ──────────────────────────────────────────────

/// The fastest round, in nanoseconds.
///
/// The fastest is the round the machine disturbed least, and it is stable
/// where a mean is not: the allocator-heavy rounds vary by half their own
/// value between runs, and a mean of those would report the load average.
fn best(samples: &[f64]) -> f64 {
    samples.iter().copied().fold(f64::MAX, f64::min)
}

/// Run `f` `ROUNDS` times, answering the fastest round in nanoseconds.
fn time(mut f: impl FnMut()) -> f64 {
    let mut samples = Vec::with_capacity(ROUNDS);
    for _ in 0..ROUNDS {
        let t = Instant::now();
        f();
        samples.push(t.elapsed().as_nanos() as f64);
    }
    best(&samples)
}

/// Write one byte per cache line of a buffer larger than this machine's L3, so
/// the next read of either form comes from memory.
///
/// Both forms fit in a 64 MiB L3, so a repeated walk measures an L3-resident
/// read. That is the right model for the emitter, which reads LIR the lowerer
/// just wrote; it is the wrong one for a JIT worker reading a function whose
/// last use was thousands of compiles ago.
fn evict(buf: &mut [u8]) {
    for i in (0..buf.len()).step_by(64) {
        buf[i] = buf[i].wrapping_add(1);
    }
    std::hint::black_box(&buf[0]);
}

/// What a set of rows holds in total.
fn total(rows: &[size::Row]) -> usize {
    rows.iter().map(|r| r.bytes).sum()
}

fn report(name: &str, per_round_ns: f64, instrs: usize) {
    println!(
        "  {name:<34} {:>9.2} ms   {:>7.1} ns/instr",
        per_round_ns / 1e6,
        per_round_ns / instrs as f64,
    );
}

fn main() {
    println!();
    println!("region-native LIR parity — the prototype against the shipped form");
    println!();

    let mut rt = Runtime::new();
    let corpus = corpus(&mut rt);
    let instrs: usize = corpus.iter().map(|f| ops::census_rust(f).0).sum();
    let uses: usize = corpus.iter().map(|f| ops::census_rust(f).1).sum();
    let blocks: usize = corpus.iter().map(|f| f.blocks.len()).sum();

    println!(
        "  corpus: {} functions, {} blocks, {} instructions, {} register operands",
        corpus.len(),
        blocks,
        instrs,
        uses,
    );
    println!(
        "  node:   SpannedInstr {} bytes (LirInstr {} + Span {}), prototype PInstr {} bytes",
        std::mem::size_of::<elle::lir::SpannedInstr>(),
        std::mem::size_of::<elle::lir::LirInstr>(),
        std::mem::size_of::<elle::syntax::Span>(),
        std::mem::size_of::<node::PInstr>(),
    );
    println!(
        "  block:  BasicBlock {} bytes, prototype PBlock {} bytes",
        std::mem::size_of::<elle::lir::BasicBlock>(),
        std::mem::size_of::<node::PBlock>(),
    );
    println!(
        "  of {} instructions, {} carry the operand vector LirInstr is sized for",
        instrs,
        size::vector_bearing(&corpus),
    );
    println!();

    // One region build, kept for the read-side measurements and for the census
    // that proves the two forms carry the same graph.
    let region = rt.heap().new_runtime_region();
    let mut builder = Builder::new(rt.heap(), region);
    let built: Vec<PFunc> = corpus.iter().map(|f| builder.func(f)).collect();
    let r_instrs: usize = built.iter().map(|f| ops::census_region(f).0).sum();
    let r_uses: usize = built.iter().map(|f| ops::census_region(f).1).sum();
    assert_eq!(
        (instrs, uses),
        (r_instrs, r_uses),
        "the prototype dropped part of the graph"
    );
    println!("  census: both forms hold {instrs} instructions and {uses} operands");
    println!();

    // Build, then release, inside one round: a growing heap of retained copies
    // measures the allocator's growth rather than the representation, and a
    // round that ends where it started measures neither.
    //
    // Both sides read the same Rust-heap corpus, so the traversal is common to
    // them. The Rust side then builds the way the lowerer does, a vector per
    // block and a push per instruction; the region side fills one reused
    // buffer and copies it into the region once.
    let mut sink: Vec<PFunc> = Vec::with_capacity(corpus.len());
    let (mut rust_builds, mut rust_teardowns) = (Vec::new(), Vec::new());
    for _ in 0..ROUNDS {
        let t0 = Instant::now();
        let copy = ops::build_rust(&corpus);
        let t1 = Instant::now();
        drop(copy);
        let t2 = Instant::now();
        rust_builds.push((t1 - t0).as_nanos() as f64);
        rust_teardowns.push((t2 - t1).as_nanos() as f64);
    }
    let rust_build = best(&rust_builds);
    let rust_teardown = best(&rust_teardowns);

    let (mut region_builds, mut region_teardowns) = (Vec::new(), Vec::new());
    for _ in 0..ROUNDS {
        let r = rt.heap().new_runtime_region();
        builder.retarget(r);
        sink.clear();
        let t0 = Instant::now();
        for f in &corpus {
            let pf = builder.func(f);
            sink.push(pf);
        }
        let t1 = Instant::now();
        rt.heap().decref_region_if_present(r);
        let t2 = Instant::now();
        region_builds.push((t1 - t0).as_nanos() as f64);
        region_teardowns.push((t2 - t1).as_nanos() as f64);
    }
    let region_build = best(&region_builds);
    let region_teardown = best(&region_teardowns);

    // The allocator traffic of one build, and what the result holds while it
    // is live, each counted on a round of its own.
    let (a0, b0) = allocs();
    let live0 = live_bytes();
    let held = ops::build_rust(&corpus);
    let rust_resident = live_bytes() - live0;
    let (a1, b1) = allocs();
    // The accounting reads the build the resident number was taken around, so
    // the two describe one object rather than two copies of a corpus whose
    // vectors grew differently.
    let rust_rows = size::rust_rows(&held);
    drop(held);
    let rust_build_allocs = a1 - a0;
    let rust_build_bytes = b1 - b0;

    let (a2, b2) = allocs();
    let pages0 = rt.heap().allocated_bytes();
    let r = rt.heap().new_runtime_region();
    builder.retarget(r);
    sink.clear();
    for f in &corpus {
        let pf = builder.func(f);
        sink.push(pf);
    }
    let region_resident = rt.heap().allocated_bytes() - pages0;
    let (a3, b3) = allocs();
    rt.heap().decref_region_if_present(r);
    let region_build_allocs = a3 - a2;
    let region_build_bytes = b3 - b2;

    // The copy a JIT promotion makes: the Rust side deep-clones a function, the
    // region side copies its slices into a fresh region.
    let mut rust_clones = Vec::new();
    for _ in 0..ROUNDS {
        let t = Instant::now();
        let mut kept: Vec<LirFunction> = Vec::with_capacity(corpus.len());
        for f in &corpus {
            kept.push(f.clone());
        }
        rust_clones.push(t.elapsed().as_nanos() as f64);
        drop(kept);
    }
    let rust_clone_ns = best(&rust_clones);

    let mut region_clones = Vec::new();
    for _ in 0..ROUNDS {
        let r = rt.heap().new_runtime_region();
        builder.retarget(r);
        sink.clear();
        let t = Instant::now();
        for f in &built {
            let c = builder.copy(f);
            sink.push(c);
        }
        region_clones.push(t.elapsed().as_nanos() as f64);
        rt.heap().decref_region_if_present(r);
    }
    let region_clone_ns = best(&region_clones);

    // Read. One warm-up pass each, so the first round's cold misses are not
    // charged to whichever form runs first.
    for f in &corpus {
        std::hint::black_box(ops::walk_rust(f));
    }
    for f in &built {
        std::hint::black_box(ops::walk_region(f));
    }
    let rust_walk = time(|| {
        let mut sum = 0u64;
        for f in &corpus {
            sum = sum.wrapping_add(ops::walk_rust(f));
        }
        std::hint::black_box(sum);
    });
    let region_walk = time(|| {
        let mut sum = 0u64;
        for f in &built {
            sum = sum.wrapping_add(ops::walk_region(f));
        }
        std::hint::black_box(sum);
    });

    // The same read, from memory rather than from L3.
    let mut scrub = vec![0u8; 128 << 20];
    let mut rust_cold = Vec::new();
    let mut region_cold = Vec::new();
    for _ in 0..ROUNDS {
        evict(&mut scrub);
        let t = Instant::now();
        let mut sum = 0u64;
        for f in &corpus {
            sum = sum.wrapping_add(ops::walk_rust(f));
        }
        rust_cold.push(t.elapsed().as_nanos() as f64);
        std::hint::black_box(sum);

        evict(&mut scrub);
        let t = Instant::now();
        let mut sum = 0u64;
        for f in &built {
            sum = sum.wrapping_add(ops::walk_region(f));
        }
        region_cold.push(t.elapsed().as_nanos() as f64);
        std::hint::black_box(sum);
    }
    let rust_walk_cold = best(&rust_cold);
    let region_walk_cold = best(&region_cold);
    drop(scrub);

    // Rewrite. Each round runs over a working copy built outside the timer, so
    // the pass always meets un-rewritten instructions.
    let mut rust_rewrites = Vec::new();
    let mut rust_hits = 0;
    for _ in 0..ROUNDS {
        let mut work: Vec<LirFunction> = corpus.to_vec();
        let t = Instant::now();
        let mut hits = 0;
        for f in &mut work {
            hits += ops::rewrite_rust(f);
        }
        rust_rewrites.push(t.elapsed().as_nanos() as f64);
        rust_hits = hits;
    }
    let rust_rewrite_ns = best(&rust_rewrites);

    let mut region_rewrites = Vec::new();
    let mut region_hits = 0;
    for _ in 0..ROUNDS {
        let r = rt.heap().new_runtime_region();
        builder.retarget(r);
        sink.clear();
        for f in &corpus {
            let pf = builder.func(f);
            sink.push(pf);
        }
        let t = Instant::now();
        let mut hits = 0;
        for f in &sink {
            hits += ops::rewrite_region(f);
        }
        region_rewrites.push(t.elapsed().as_nanos() as f64);
        region_hits = hits;
        rt.heap().decref_region_if_present(r);
    }
    let region_rewrite_ns = best(&region_rewrites);
    assert_eq!(
        rust_hits, region_hits,
        "the two rewrites converted different instruction counts"
    );
    println!("  rewrite: both passes convert {rust_hits} ValueConst instructions");
    println!();

    println!("  Rust-heap LirFunction");
    report("build (the lowerer's pushes)", rust_build, instrs);
    report("copy (a JIT promotion)", rust_clone_ns, instrs);
    report("walk (a backend's read)", rust_walk, instrs);
    report("walk, from memory", rust_walk_cold, instrs);
    report("rewrite (the send pass)", rust_rewrite_ns, instrs);
    report("teardown", rust_teardown, instrs);
    println!(
        "    {} malloc calls, {} KiB requested per build",
        rust_build_allocs,
        rust_build_bytes / 1024,
    );
    println!(
        "    {} KiB held while live (Rust heap)",
        rust_resident / 1024
    );
    println!();

    let region_rows = size::region_rows(&built);
    let region_total = total(&region_rows);

    println!("  region prototype");
    report("build (encode into a region)", region_build, instrs);
    report("copy (a JIT promotion)", region_clone_ns, instrs);
    report("walk (a backend's read)", region_walk, instrs);
    report("walk, from memory", region_walk_cold, instrs);
    report("rewrite (the send pass)", region_rewrite_ns, instrs);
    report("teardown (free the region)", region_teardown, instrs);
    println!(
        "    {} malloc calls, {} KiB requested per build",
        region_build_allocs,
        region_build_bytes / 1024,
    );
    println!(
        "    {} KiB held while live (region pages), {} KiB of it payload",
        region_resident / 1024,
        region_total / 1024,
    );
    println!();

    // Where the two totals differ, row by row. The Rust rows count `capacity`,
    // so a vector's growth slack lands in the row that grew it.
    println!("  where the bytes go (KiB)");
    for row in &rust_rows {
        let region: usize = region_rows
            .iter()
            .filter(|r| r.what == row.what)
            .map(|r| r.bytes)
            .sum();
        println!(
            "  {:<34} {:>9} {:>9}",
            row.what,
            row.bytes / 1024,
            region / 1024
        );
    }
    for row in &region_rows {
        if !rust_rows.iter().any(|r| r.what == row.what) {
            println!("  {:<34} {:>9} {:>9}", row.what, 0, row.bytes / 1024);
        }
    }
    println!(
        "  {:<34} {:>9} {:>9}",
        "total",
        total(&rust_rows) / 1024,
        region_total / 1024
    );
    println!(
        "  {:<34} {:>9} {:>9}",
        "page slack the slices do not name",
        0,
        region_resident.saturating_sub(region_total) / 1024,
    );
    println!();

    rt.heap().decref_region_if_present(region);
    rt.teardown();
}
