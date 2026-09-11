// audited: 2026-09-11
// docs/impl/image/sealing.md
//! The dynamic-parameter id counter: the one mint in the process, and the
//! watermark a hydrated image raises it past.
//!
//! A parameter is resolved by id — a fiber's parameter frames name the
//! parameter they rebind by that number, and so does every closure that
//! captured it. Two live parameters sharing an id therefore rebind each other,
//! which is why every id in the process comes from here.

use std::sync::atomic::{AtomicU32, Ordering};

static NEXT_ID: AtomicU32 = AtomicU32::new(0);

/// The id for a parameter allocated now.
pub(crate) fn mint() -> u32 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// The id the next [`mint`] hands out — the observation point for a test that
/// asks where the counter stands.
#[cfg(test)]
pub(crate) fn peek() -> u32 {
    NEXT_ID.load(Ordering::Relaxed)
}

/// Raise the counter so that no later mint answers below `watermark`.
///
/// A hydrated image's body carries the ids the dumping process handed out, and
/// this instance's counter knows nothing about them. The compare-exchange loop
/// is what keeps the raise from lowering a counter another thread just pushed
/// higher.
pub(crate) fn raise_to(watermark: u32) {
    let mut seen = NEXT_ID.load(Ordering::Relaxed);
    while seen < watermark {
        match NEXT_ID.compare_exchange_weak(seen, watermark, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return,
            Err(actual) => seen = actual,
        }
    }
}
