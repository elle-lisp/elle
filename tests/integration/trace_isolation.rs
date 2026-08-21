// Trace state is per-instance, not process-global: a `--trace=`-heavy corpus file
// must not bleed its tracing into the rest of a shared `elle test` run. Each test
// file runs in its own worker VM (its own `FiberHeap`, its own trace cell), and
// the off-VM trace readers (region-page claims, channels, POSIX) each read their
// instance's cell — so a file that enables `:pages` and never clears it cannot
// make a *different* file's page allocations (or the runner's own main-thread
// machinery) spam `[trace:pages]`.
//
// Before the relocation, `(vm/config-set :trace :pages)` wrote a process-global
// atomic that every region pool in the process read, so one offending file made
// the whole run crawl. This fixture reproduces that shape: file P enables `:pages`
// and aborts before clearing; file Q (a separate instance) allocates heavily. With
// the per-instance cell, the only `[trace:pages]` lines are P's own — Q's
// allocations and the runner's processing stay silent.

use std::process::Command;

fn elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

/// Count `[trace:pages]` lines an `elle test` run emits on stderr.
fn pages_trace_lines(files: &[&std::path::Path]) -> usize {
    let cache = crate::common::ScratchDir::new("trace-iso-cache");
    // The session DB (and its sibling CAS dir) live in the scratch dir; no
    // ELLE_CACHE needed — `--db` names the path outright, as `truncation.rs` does.
    let out = Command::new(elle_binary())
        .arg("test")
        .args(files)
        .args(["--timeout", "30000"])
        .arg("--db")
        .arg(cache.join("s.db"))
        .env_remove("RUST_MIN_STACK")
        .output()
        .expect("run elle test");
    String::from_utf8_lossy(&out.stderr)
        .lines()
        .filter(|l| l.contains("[trace:pages]"))
        .count()
}

#[test]
fn pages_trace_does_not_bleed_across_instances() {
    let dir = crate::common::ScratchDir::new("trace-iso");
    // P: enable :pages, then abort before clearing it — the offending file leaves
    // its trace bit set for the rest of *its* instance's life.
    let p = dir.join("p_enables_pages.lisp");
    std::fs::write(
        &p,
        "(vm/config-set :trace |:pages|)\n\
         (assert (= 1 2) \"P aborts with :pages still enabled\")\n",
    )
    .unwrap();
    // Q: a separate instance that allocates heavily (many region-page claims). Its
    // own trace cell never has :pages, so its page claims must stay silent.
    let q = dir.join("q_allocates.lisp");
    std::fs::write(
        &q,
        "(defn build [n] (if (= n 0) [] (concat [n] (build (- n 1)))))\n\
         (assert (= (length (build 400)) 400) \"Q allocates a lot\")\n",
    )
    .unwrap();

    // Control: Q on its own never enables :pages, so it emits nothing.
    let q_alone = pages_trace_lines(&[&q]);
    assert_eq!(
        q_alone, 0,
        "an allocating file that never enables :pages must emit no [trace:pages] \
         lines; got {}",
        q_alone
    );

    // The run: P (sets :pages, aborts) then Q (allocates). Post-relocation the only
    // [trace:pages] lines are P's own instance (~60 here); Q — a separate instance
    // — contributes nothing, and the runner's main-thread page allocations are not
    // traced either. Before the relocation the process-global bit made this ~600+.
    let together = pages_trace_lines(&[&p, &q]);
    assert!(
        together < 250,
        "a file enabling :pages must not bleed into other instances in a shared \
         run — expected the total well below the pre-relocation ~600 (only P's own \
         instance should trace), got {}",
        together
    );
}
