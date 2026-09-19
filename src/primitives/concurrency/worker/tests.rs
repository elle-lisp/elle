//! Unit tests (`super` is the parent impl module).

use super::*;

#[test]
fn rust_min_stack_overrides_main_thread_limit() {
    // An explicit RUST_MIN_STACK wins over the rlimit, like std itself.
    let n = resolve_worker_stack(Some("16777216"), Some(8 * 1024 * 1024));
    assert_eq!(n, 16 * 1024 * 1024);
}

#[test]
fn worker_matches_main_thread_limit_by_default() {
    // No env override → match the main thread's stack so the worker can
    // compile anything main can.
    let main: u64 = 12 * 1024 * 1024;
    assert_eq!(resolve_worker_stack(None, Some(main)), main as usize);
}

#[test]
fn unbounded_or_unreadable_limit_uses_fallback() {
    // RLIMIT_STACK == infinity / unreadable is surfaced as None; we don't
    // try to reserve an unbounded stack — a fixed fallback instead.
    assert_eq!(resolve_worker_stack(None, None), WORKER_STACK_FALLBACK);
}

#[test]
fn never_below_the_floor() {
    // A tiny rlimit or a tiny RUST_MIN_STACK is clamped up — below the
    // floor invites overflow in the runtime itself.
    assert_eq!(
        resolve_worker_stack(None, Some(64 * 1024)),
        WORKER_STACK_FLOOR
    );
    assert_eq!(
        resolve_worker_stack(Some("65536"), None),
        WORKER_STACK_FLOOR
    );
}

#[test]
fn never_above_the_cap() {
    // A pathologically large limit (or env) is capped — no absurd per-worker
    // reservation.
    let huge = 1024u64 * 1024 * 1024; // 1 GiB
    assert_eq!(resolve_worker_stack(None, Some(huge)), WORKER_STACK_CAP);
    assert_eq!(
        resolve_worker_stack(Some("1073741824"), None),
        WORKER_STACK_CAP
    );
}

#[test]
fn garbage_env_is_ignored_and_falls_through() {
    // A non-numeric RUST_MIN_STACK is ignored, not fatal — fall through to
    // the main-thread limit.
    let main: u64 = 8 * 1024 * 1024;
    assert_eq!(
        resolve_worker_stack(Some("not-a-number"), Some(main)),
        main as usize
    );
}
