// audited: 2026-09-21
// docs/impl/wasm.md
//! The capability gate on this tier: a native the calling fiber withholds is
//! denied rather than run, and the fiber reads the shared denial payload.
//!
//! The trap every case here is built around: a body that reaches a primitive
//! THROUGH a prelude function runs that function on the host VM, whose own gate
//! then fires. Such a shape passes with this tier's gate missing entirely, so
//! each case names a native the fiber body calls directly. Written with
//! `println`, these tests are green against an ungated host.
//!
//! Each expression evaluates to a boolean so the assertion reads the language's
//! answer rather than a value's display form.

use super::*;

/// The ordinary call path. `do` keeps the call mid-activation.
#[test]
fn wasm_full_denies_a_call_position_native_the_fiber_withholds() {
    assert_eq!(
        eval_with_stdlib(
            "(let [f (fiber/new (fn [] (do (path/exists? \"blocked\") 1)) \
                                |:error :fs| :deny |:fs|)] \
               (fiber/resume f) \
               (= :paused (fiber/status f)))"
        ),
        "true",
        "a call-position native the fiber withholds must park it, not run"
    );
}

/// The tail path is a host path of its own and reaches its own native.
#[test]
fn wasm_full_denies_a_tail_position_native_the_fiber_withholds() {
    assert_eq!(
        eval_with_stdlib(
            "(let [f (fiber/new (fn [] (path/exists? \"blocked\")) \
                                |:error :fs| :deny |:fs|)] \
               (fiber/resume f) \
               (= :paused (fiber/status f)))"
        ),
        "true",
        "a tail-position native the fiber withholds must park it, not run"
    );
}

/// The payload is the shared `{:error :capability-denied …}` struct, so a
/// program reads the same fields whichever tier ran its body.
#[test]
fn wasm_full_denial_carries_the_shared_payload() {
    assert_eq!(
        eval_with_stdlib(
            "(let [f (fiber/new (fn [] (do (path/exists? \"blocked\") 1)) \
                                |:error :fs| :deny |:fs|)] \
               (fiber/resume f) \
               (let [v (fiber/value f)] \
                 (and (= :capability-denied (get v :error)) \
                      (= \"path/exists?\" (get v :primitive)) \
                      (not (nil? ((get v :denied) :fs))))))"
        ),
        "true",
        "the denial payload must name the denied bit and the primitive"
    );
}

/// The gate reads the withheld set, not the primitive's declared bits.
#[test]
fn wasm_full_runs_a_native_the_fiber_still_holds() {
    assert_eq!(
        eval_with_stdlib(
            "(let [f (fiber/new (fn [] (do (path/exists? \"no-such-path\") :ran)) \
                                |:error :fs|)] \
               (fiber/resume f) \
               (and (= :dead (fiber/status f)) (= :ran (fiber/value f))))"
        ),
        "true",
        "a fiber withholding nothing must run its native to completion"
    );
}

/// Denying an unrelated bit leaves the call alone, so the gate is not a
/// blanket refusal of every native on a restricted fiber.
#[test]
fn wasm_full_leaves_a_native_the_denial_does_not_name() {
    assert_eq!(
        eval_with_stdlib(
            "(let [f (fiber/new (fn [] (do (path/exists? \"no-such-path\") :ran)) \
                                |:error :fs :exec| :deny |:exec|)] \
               (fiber/resume f) \
               (and (= :dead (fiber/status f)) (= :ran (fiber/value f))))"
        ),
        "true",
        "denying :exec must not block a call that needs only :fs"
    );
}

/// The argument-derived requirement this branch adds is asked on this tier
/// too: `io/submit` declares `:error` alone and derives the rest from the
/// request's operation, so a tier gating on declared bits submits the spawn.
#[test]
fn wasm_full_denies_a_submit_for_the_operation_its_request_carries() {
    assert_eq!(
        eval_with_stdlib(
            "(let [req (fiber/resume \
                         (fiber/new (fn [] (subprocess/exec \"/bin/sh\" [\"-c\" \"true\"])) \
                                    |:error :io :exec|)) \
                   f (fiber/new (fn [r] (io/submit (io/backend :async) r)) \
                                |:error :io :exec| :deny |:exec|)] \
               (fiber/resume f req) \
               (and (= :paused (fiber/status f)) \
                    (= :capability-denied (get (fiber/value f) :error))))"
        ),
        "true",
        "io/submit must be denied for the :exec its request carries, not for \
         the :error it declares"
    );
}
