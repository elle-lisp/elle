//! Process-teardown contract (docs/impl/region/rules.md § "Teardown — every
//! region frees").
//!
//! `elle foo.lisp` ≡ `(eval (wrap-in-letrec (read-all (slurp "foo.lisp"))))`:
//! after the run completes and its result is dropped, the world returns to its
//! pre-`main` state — every region is reclaimed *by RC reaching zero* (roots
//! dropped → cascade), never by iterating the region table. Nothing survives in
//! a region: native-fns are immediates (no region, no heap cell).
//!
//! All three entry paths (file run, REPL exit, embedding) drive one `Runtime`,
//! so this test exercises the embedding path and thereby the shared teardown
//! every path uses.
//!
//! This is its own test binary so its repeated full-`Runtime` build/teardown
//! cycles and residue inspection stay isolated from the shared integration-test
//! binary.

use elle::pipeline::compile_file;
use elle::runtime::Runtime;

#[path = "region_process_teardown/census.rs"]
mod census;
#[path = "region_process_teardown/growth.rs"]
mod growth;

fn census(rt: Runtime, src: &str) {
    census_with(rt, Some(src));
}

fn census_with(mut rt: Runtime, src: Option<&str>) {
    if let Some(src) = src {
        // `value` is `Copy`; scope the disjoint parts() borrows so the heap
        // borrow for the root registration below does not alias them.
        let value = {
            let (vm, symbols, cctx) = rt.parts();
            let result = compile_file(src, symbols, cctx, "<census>").expect("compiles");
            vm.execute_scheduled(&result.bytecode, symbols, cctx)
                .expect("runs")
        };
        // The program value is handed to the embedding caller with one owning
        // reference ("ownership transfer"). Dropping the `Value` is a no-op (it
        // is `Copy`); the caller owes a release. Routing it through the
        // process-root registry lets the teardown sweep consume it.
        // `register_process_root` takes the heap first.
        elle::value::arena::register_process_root(rt.heap(), value);
    }
    let report = rt.teardown();
    report_census(rt.heap(), &report);
}

fn report_census(heap: &elle::value::fiberheap::FiberHeap, report: &elle::runtime::TeardownReport) {
    eprintln!("census: {} regions survive teardown", report.live_regions);
    // For every surviving region, count how many of its rc references are
    // explained by another live region's contents (in-degree). The remainder
    // (`rc - in_degree`) is an owner/escape reference that was never released —
    // those regions are the *roots* of the leak graph; everything else is
    // their cascade shadow. Group the roots by tag signature. Reads this
    // instance's own heap (passed explicitly).
    let mut indegree: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
    for (_, to) in heap.cross_ref_edges() {
        *indegree.entry(to).or_insert(0) += 1;
    }
    let mut root_classes: std::collections::HashMap<String, (usize, u32)> =
        std::collections::HashMap::new();
    let mut shadow = 0usize;
    for &(id, rc, _objs) in &report.regions {
        let ind = indegree.get(&id).copied().unwrap_or(0);
        if rc > ind {
            let mut tags: Vec<String> = heap
                .region_tags(id)
                .iter()
                .map(|t| format!("{t:?}"))
                .collect();
            tags.sort();
            tags.dedup();
            let e = root_classes.entry(tags.join("+")).or_insert((0, 0));
            e.0 += 1;
            e.1 += rc - ind;
        } else {
            shadow += 1;
        }
    }
    let mut classes: Vec<_> = root_classes.into_iter().collect();
    classes.sort_by_key(|(_, (n, _))| std::cmp::Reverse(*n));
    eprintln!("leak-graph roots by tag class (regions, unexplained refs):");
    for (tags, (n, ext)) in &classes {
        eprintln!("  {n:>6} regions  {ext:>6} unexplained refs  [{tags}]");
    }
    eprintln!("  {shadow:>6} regions fully explained by live cross-refs (cascade shadow)");
    if report.regions.len() <= 64 {
        heap.debug_dump();
        eprintln!("edges: {:?}", heap.cross_ref_edges());
    }
}

/// Live-region population bucketed by tag class (sorted+deduped tag signature),
/// read from the current heap WITHOUT a teardown. The growth analogue of
/// `report_census`: snapshot this, do work, snapshot again, diff the buckets.
fn region_class_histogram(
    heap: &elle::value::fiberheap::FiberHeap,
) -> std::collections::BTreeMap<String, usize> {
    let mut hist: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for (id, _rc, _objs) in heap.region_info_vec() {
        if id == 1 {
            continue; // retired id 1 — never minted, never a leak
        }
        let mut tags: Vec<String> = heap
            .region_tags(id)
            .iter()
            .map(|t| format!("{t:?}"))
            .collect();
        tags.sort();
        tags.dedup();
        *hist.entry(tags.join("+")).or_insert(0) += 1;
    }
    hist
}

fn per_compile_growth_one(label: &str, src: &str) {
    let mut rt = Runtime::new();
    let n: usize = 20;
    let (base, after) = {
        let (vm, symbols, cctx) = rt.parts();
        // One warm-up compile absorbs any one-time lazy init, so the delta is
        // steady-state per-compile residue, not first-call setup.
        let _ = compile_file(src, symbols, cctx, "<census>").expect("compiles");
        let base = region_class_histogram(vm.heap());
        for _ in 0..n {
            let _ = compile_file(src, symbols, cctx, "<census>").expect("compiles");
        }
        let after = region_class_histogram(vm.heap());
        (base, after)
    };

    let mut total: i64 = 0;
    let mut classes: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    classes.extend(base.keys().cloned());
    classes.extend(after.keys().cloned());
    let mut parts: Vec<String> = Vec::new();
    for class in &classes {
        let b = *base.get(class).unwrap_or(&0) as i64;
        let a = *after.get(class).unwrap_or(&0) as i64;
        if a != b {
            total += a - b;
            parts.push(format!("{}+{}", class, (a - b) as f64 / n as f64));
        }
    }
    eprintln!(
        "{label} {:>5.1}/compile  [{}]",
        total as f64 / n as f64,
        parts.join(", ")
    );
    let _ = rt.teardown();
}
