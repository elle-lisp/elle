// audited: 2026-09-28
// Guardfree pins for io parks: the request an install releases, and the port and buffer beside it.
//
// docs/impl/region/park.md
// docs/impl/io-bytes.md

use super::*;

// Guard — the other park a payload the RUNTIME built (docs/impl/region/park.md):
// a yielding io op's `IoRequest`, which the native built and the body never
// named, so no continuation releases it. Every install
// that displaces the park owes that release, and `fiber/abort` / `fiber/refuse`
// each run one where none ran before. The mediator reads the request out of the
// park before it ends it — `fiber/value` is pass-through, so a binding carries a
// counted reference of its own — and every witness DEREFERENCES the request after
// the install; a bare status check passes over a freed one. The `:io` denial
// witnesses are the bits collision: a fiber denied `:io` parks under `SIG_IO`, so
// the ledger record and the io bit both answer for one park and exactly one
// reference is owed. Running both frees the payload under the mediator's read —
// SIGSEGV under guardfree. The leak face is `tests/elle/region-io-park.lisp`.
#[test]
fn region_io_park_uaf() {
    run_elle_script_with_args(
        "region-io-park-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a JIT-compiled fiber that suspends mid-I/O must not over-release the
// yielded io-request region. `--mlir=off` pins the pure-JIT path (the invariant
// must not depend on the MLIR backend being present); the harness's vm/jit
// policies don't isolate this combination on an MLIR-enabled build.
#[test]
fn region_jit_io_suspend_uaf() {
    run_elle_script_with_args("region-jit-io-suspend-uaf", &["--mlir=off"]);
}

// Guard — an io completion struct shares the reaping call's region, so the
// scheduler pump's release of the `io/wait` array cascades to the payload the
// backend built and handed the resumed fiber. That fiber's own reference is
// what must carry the payload past the cascade; under the UAF oracle a missing
// one faults at the read instead of returning a recycled page.
#[test]
fn region_io_completion_leak_guardfree() {
    run_elle_script_with_args(
        "region-io-completion-leak",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a `Fresh` io op builds its completion buffer in the request's own
// region, and the install that ends the park releases the suspend retain there
// like any other. The buffer the resume hands back must survive that release:
// under the UAF oracle a release that took one reference too many faults at the
// read of a held chunk instead of returning it.
#[test]
fn region_io_read_strand_guardfree() {
    run_elle_script_with_args(
        "region-io-read-strand",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a fiber that relays a child's io park with `(emit :io v)` parks under
// `SIG_IO` with an `IoRequest` it did not build, so the install that answers the
// relay owes that request nothing (docs/impl/region/park.md). The request's
// region also holds what the child reads next: the port `port/open` answers
// with, the buffer a read answers in. Each relaying install that releases it
// frees those under the child. The witnesses relay once, twice, and through a
// relay that reads each request after its install, and the child reads its port
// and lines, closes, writes and reads the file back. A stale read faults —
// SIGSEGV under guardfree. The same file gauges the leak a relay must not trade
// the fault for.
#[test]
fn region_io_relay_uaf() {
    run_elle_script_with_args(
        "region-io-relay-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}
