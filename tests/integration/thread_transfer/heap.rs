// A worker thread's heap dies with the thread.
//
// `os/spawn` stands a whole instance up on the worker: a VM, a symbol table, a
// compile context, and the region heap every value it builds lives in. The
// result crosses back as a serialized bundle — a deep copy — so nothing the
// worker allocated is reachable after the join. If the worker's heap outlives
// its thread, a program that runs workers in sequence pays for every one of
// them at once, and it pays whatever each worker left live: the `elle test`
// runner ships each corpus file to its own worker (twice, once per JIT policy),
// and a 25-file batch peaked at 13 GB that way.
//
// The gauge is `mapped_bytes` — bytes the region page pools hold from the OS,
// process-wide. `arena/page-claims` cannot serve here: it reads one heap's
// claims, and the heap in question belongs to another thread.
//
// The measurement is a SLOPE, for the same reason the leak dashboard is: what
// a worker costs while it runs is not a defect, and the caller's own heap moves
// under the window. Growth per additional worker is the defect. Before the
// worker owned its heap (`VM::new` leaks one by design, for VMs whose values
// must outlive them), each heavy worker left its whole instance mapped —
// ~7 MB of stdlib apiece, plus anything the body left live.

use super::*;
use elle::value::fiberheap::mapped_bytes;

/// Spawn `n` heavy workers one after another, joining each before the next.
/// Heavy (`os/spawn`, not `os/spawn-vm`) is the runner's shape: the worker
/// loads its own stdlib, which is the bulk of what it maps.
fn run_workers(n: usize) {
    let source = format!(
        r#"(let [total @[]]
             (each i in (range 0 {n})
               (push total (os/join (os/spawn (fn [] (length (range 0 64)))))))
             (length total))"#
    );
    eval_source(&source, |r| {
        let v = r.expect("the workers ran and joined");
        assert_eq!(v.as_int(), Some(n as i64), "every worker was joined");
    });
}

#[test]
fn worker_heap_returns_its_pages_at_join() {
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
