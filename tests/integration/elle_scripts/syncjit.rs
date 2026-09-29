// audited: 2026-09-29
// Corpus files run with the JIT compiling on the VM thread (`--trace=syncjit`).
//
// docs/impl/jit.md

use super::*;

// `--trace=syncjit` is a process-global trace, so the runner cannot turn it on
// for one file; these pins are the only way the corpus meets it.
//
// What it buys is a schedule rather than a feature. The background worker
// installs a compile some unbounded time after the call that asked for it, so
// anything that depends on WHEN code is installed reads whatever the worker's
// queue happened to do. Syncjit installs before the submitting call returns,
// which fixes that schedule at one end of its range: a caller is compiled from
// its first call onward, and every later call it makes runs as native code.
//
// Codegen inputs are identical either way (same `prepare_task` output), so a
// disagreement between these runs and the runner's `--jit=eager` pass is never
// about codegen. It is about what the tier does once code is installed, which
// is exactly what an eager-only corpus cannot see.
const SYNCJIT: &[&str] = &["--jit=eager", "--trace=syncjit"];

// A caller compiled on its first call reaches its callees through
// `elle_jit_call` from then on. These two files call their hot functions
// indirectly through a `repeat2` helper, so under syncjit that helper is
// compiled before it ever passes `push-imm-hot` along — and the file's own
// `(jit? push-imm-hot)` is the reading that says whether the compiled call
// path promotes what it calls.
#[test]
fn jit_bytes_push_syncjit() {
    run_elle_script_with_args("jit-bytes-push", SYNCJIT);
}

#[test]
fn jit_string_push_syncjit() {
    run_elle_script_with_args("jit-string-push", SYNCJIT);
}

// The same claim with the schedule written into the file rather than into the
// flags: the corpus file drains between the two loops, so it holds under the
// runner's policies too. Pinned here as well because syncjit is the shape that
// leaves the compiled caller no interpreted window at all.
#[test]
fn compiled_caller_promotes_callee_syncjit() {
    run_elle_script_with_args("jit-compiled-caller-promotes-callee", SYNCJIT);
}

// A compiled function whose tail call suspends parks its continuation there,
// so the resume runs the releases after the tail call
// (docs/impl/region/park.md). Under syncjit each face runs compiled from its
// first call, so the gauge reads compiled tail calls alone. The guardfree run
// faults where a replayed release frees an argument its caller still reads.
#[test]
fn region_jit_tail_suspend_syncjit() {
    run_elle_script_with_args("region-jit-tail-suspend", SYNCJIT);
}

#[test]
fn region_jit_tail_suspend_guardfree() {
    run_elle_script_with_args(
        "region-jit-tail-suspend",
        &["--jit=eager", "--trace=syncjit,guardfree"],
    );
}
