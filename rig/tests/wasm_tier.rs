// audited: 2026-09-29
// Under a sidecar's `wasm = N`, a hot closure runs on the tiered backend from its Nth call.
// docs/impl/wasm.md
// rig/overview.md

#![cfg(feature = "wasm")]

mod common;

use common::{rig, stderr, stdout, Scratch};

/// The line `--trace=wasm` prints when the tier compiles `id`.
const COMPILED_ID: &str = "[wasm-tier] compiled Some(\"id\")";

/// Run a file that calls a leaf closure `calls` times, under `wasm = n` with
/// the tier's trace armed, and answer the rig's stderr.
fn run(n: u32, calls: u32) -> String {
    let dir = Scratch::new("wasmtier");
    let path = dir.write(
        "tier.lisp",
        &format!(
            "(elle/epoch 13)\n\
             (defn id [x] x)\n\
             (each i in (range {calls}) (id i))\n\
             (println \"done\")\n"
        ),
    );
    dir.write("tier.toml", &format!("wasm = {n}\ntrace = [\"wasm\"]\n"));
    let out = rig(&[path]);
    let err = stderr(&out);
    assert!(
        out.status.success() && stdout(&out).trim() == "done",
        "the file did not run under wasm = {n}: {:?}\nstderr:\n{err}",
        out.status
    );
    err
}

// The counter-factual: the heat check read the JIT's threshold, and a wasm
// build has no JIT tier, so its threshold was unreachable and no closure ever
// compiled, whatever N said.
#[test]
fn a_closure_runs_on_the_tier_from_its_nth_call() {
    let err = run(3, 3);
    assert!(
        err.contains(COMPILED_ID),
        "under wasm = 3, three calls to `id` did not compile it:\n{err}"
    );
}

// The companion to the test above: a fix that compiled every closure on its
// first call, ignoring N, would pass it.
#[test]
fn a_closure_called_fewer_than_n_times_stays_in_the_interpreter() {
    let err = run(3, 2);
    assert!(
        !err.contains(COMPILED_ID),
        "under wasm = 3, two calls to `id` compiled it:\n{err}"
    );
}
