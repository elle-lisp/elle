// audited: 2026-10-20
//! A worker thread's heap dies with the thread.
//!
//! Its own binary: `mapped_bytes` is one process-wide counter, and the window
//! measured below is a minute and a half wide (docs/analysis/testing.md
//! § "Process-global state needs its own binary").
//
// `os/spawn` stands a whole instance up on the worker: a VM, a symbol table, a
// compile context, and the region heap every value it builds lives in. The
// result crosses back as a serialized bundle — a deep copy — so nothing the
// worker allocated is reachable after the join. If the worker's heap outlives
// its thread, a program that runs workers in sequence pays for every one of
// them at once, and it pays whatever each worker left live. The `elle test`
// runner ships each corpus file to its own worker (twice, once per JIT policy),
// and a 25-file batch peaked at 13 GB that way.
//
// The gauge is `mapped_bytes` — bytes the region page pools hold from the OS,
// process-wide. `arena/page-claims` cannot serve here: it reads one heap's
// claims, and the heap in question belongs to another thread.
//
// The measurement is a SLOPE, for the same reason the leak dashboard is: what
// a worker costs while it runs is not a defect, and the caller's own heap moves
// under the window. Growth per additional worker is the defect. The
// counter-factual is a worker heap built through `VM::new`, which leaks one by
// design for VMs whose values must outlive them. Each heavy worker then left
// its whole instance mapped: ~7 MB of stdlib apiece, plus anything the body
// left live.

#[path = "common/mod.rs"]
mod common;

use common::eval_source;
use elle::value::fiberheap::mapped_bytes;

use elle::compiler::stdlib_cache::StdlibCache;
use elle::image::boot::{BootImage, BootSource};
use elle::runtime::{BootCaches, Runtime};

/// One gauge window at a time. `mapped_bytes` counts every thread's pages, so
/// two tests measuring at once would each read the other's workers.
static GAUGE: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The program that spawns `n` heavy workers one after another, joining each
/// before the next, and answers how many it joined. Heavy (`os/spawn`, not
/// `os/spawn-vm`) is the runner's shape: the worker holds its own stdlib, which
/// is the bulk of what it maps.
fn workers_program(n: usize) -> String {
    format!(
        r#"(let [total @[]]
             (each i in (range 0 {n})
               (push total (os/join (os/spawn (fn [] (length (range 0 64)))))))
             (length total))"#
    )
}

/// Run `n` workers from a fresh runtime that boots under the process policy.
fn run_workers(n: usize) {
    eval_source(&workers_program(n), |r| {
        let v = r.expect("the workers ran and joined");
        assert_eq!(v.as_int(), Some(n as i64), "every worker was joined");
    });
}

/// Run `n` workers from `rt`.
fn run_workers_on(rt: &mut Runtime, n: usize) {
    let (vm, symbols, cctx) = rt.parts();
    let v = elle::eval_all(&workers_program(n), symbols, vm, cctx, "<test>")
        .expect("the workers ran and joined");
    assert_eq!(v.as_int(), Some(n as i64), "every worker was joined");
}

#[test]
fn worker_heap_returns_its_pages_at_join() {
    let _gauge = GAUGE.lock().unwrap_or_else(|e| e.into_inner());
    // Warm up: the caller's own stdlib is a one-time cost that must not land
    // inside the measured window.
    run_workers(1);

    let base = mapped_bytes();
    run_workers(1);
    let after_one = mapped_bytes().saturating_sub(base);
    run_workers(6);
    let after_seven = mapped_bytes().saturating_sub(base);

    // Seven workers must not cost seven times one. The slack is one worker's
    // instance; the pre-fix reading was ~7 MB per worker, so a regression
    // clears it by a wide margin.
    assert!(
        after_seven < after_one + 4 * 1024 * 1024,
        "worker heaps outlive their threads: 1 worker left {after_one} bytes \
         of region pages mapped, 7 left {after_seven} — a joined worker's heap \
         must go with it (docs/threads.md § A worker owns its heap and gives \
         it back)",
    );
}

// A worker of an image-booted instance hydrates its parent's image
// (docs/impl/image/boot.md), so the pages it maps are a private mapping of the
// image file, about 9 MB apiece. The trap is the pool: it never caches such a
// page, and only the page's own drop unmaps it, so a worker heap that drops
// without releasing it leaves the mapping behind. Seven workers must not cost
// seven mappings.
#[test]
fn an_image_booted_workers_mapping_goes_with_it_at_join() {
    let _gauge = GAUGE.lock().unwrap_or_else(|e| e.into_inner());
    let dir = common::ScratchDir::new("worker-heap-image");
    let caches = || BootCaches {
        stdlib: StdlibCache::Off,
        image: BootImage::Dir(dir.path().to_path_buf()),
    };
    drop(Runtime::with_caches(caches()));
    let mut rt = Runtime::with_caches(caches());
    assert_eq!(
        rt.boot_source(),
        BootSource::Image,
        "the instance did not hydrate the image the one before it stored"
    );
    run_workers_on(&mut rt, 1);

    let base = mapped_bytes();
    run_workers_on(&mut rt, 1);
    let after_one = mapped_bytes().saturating_sub(base);
    run_workers_on(&mut rt, 6);
    let after_seven = mapped_bytes().saturating_sub(base);

    assert!(
        after_seven < after_one + 4 * 1024 * 1024,
        "a hydrated worker's mapping outlives its thread: 1 worker left \
         {after_one} bytes mapped, 7 left {after_seven}",
    );
}
