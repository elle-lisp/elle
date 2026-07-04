//! Compile-time RC-coalescing statistics — the measured win of compile-time
//! region selection (docs/impl/region-rules.md § "Compile-time region selection
//! (coalescing)" / "Self-edge elimination").
//!
//! Every region-mint the lowerer emits at a coalescing-candidate site is either
//! **slot-resolved** — the value's region was statically nameable, so the
//! `IncrefRegion`/`DecrefRegion` names a slot and the runtime `region_of` deref
//! is saved (the win) — or **value-resolved** (`IncrefValueRegion`/
//! `DecrefValueRegion`, the honest dynamic boundary). Transform 2 additionally
//! drops a merge-induced intra-region self-edge incref (a removed leak). These
//! counters tally those decisions across a compilation so a benchmark
//! (`benches/regionrc.rs`) can report the corpus-wide reduction: the win is
//! *measured, not asserted*.
//!
//! The decision at each site is *not* recoverable from the final LIR — a
//! slot-resolved coalesced mint and an ordinary store-edge `IncrefRegion` are
//! indistinguishable instructions, and an eliminated self-edge leaves no
//! instruction at all — so the lowerer records the decision here, at the point it
//! is made.
//!
//! Thread-local, so a measuring thread reads exactly its own compilation and
//! parallel test threads never cross-contaminate. The lowerer bumps a counter
//! unconditionally at each decision site (a `Cell` add — negligible against the
//! surrounding region analysis and emission); a consumer calls [`reset`] before a
//! measured compile and [`snapshot`]/[`take`] after.

use std::cell::Cell;

/// Per-thread tally of the lowerer's compile-time region-selection decisions.
/// Each candidate site increments exactly one `*_slot` (coalesced) or `*_value`
/// (value-resolved) counter; transform 2 increments `self_edges_eliminated`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RcCoalesceStats {
    /// `lower_return`'s mint resolved to a static slot (`IncrefRegion`).
    pub return_mint_slot: u64,
    /// `lower_return`'s mint stayed value-resolved (`IncrefValueRegion`).
    pub return_mint_value: u64,
    /// The reassign incref-on-store pinned the new content via a slot
    /// (`IncrefRegion`) — a fn-local 1-slot container.
    pub reassign_store_slot: u64,
    /// The reassign incref-on-store stayed value-resolved (`IncrefValueRegion`)
    /// — a module-scope container's value, the dynamic boundary.
    pub reassign_store_value: u64,
    /// `store_captured_cell_init`'s init-drop resolved to a slot
    /// (`DecrefRegion`) — transform 1's decref side.
    pub captured_init_slot: u64,
    /// `store_captured_cell_init`'s init-drop stayed value-resolved
    /// (`DecrefValueRegion`) — an opaque call-result init.
    pub captured_init_value: u64,
    /// Merge-induced intra-region self-edges dropped (transform 2). Each is one
    /// `IncrefRegion` the free-time cascade would never balance — a removed leak.
    pub self_edges_eliminated: u64,
}

impl RcCoalesceStats {
    /// Candidate mints (across all three transform-1 sites) that resolved to a
    /// static slot — the value→slot win.
    pub fn coalesced(&self) -> u64 {
        self.return_mint_slot + self.reassign_store_slot + self.captured_init_slot
    }

    /// Candidate mints that stayed value-resolved — the honest dynamic boundary.
    pub fn value_resolved(&self) -> u64 {
        self.return_mint_value + self.reassign_store_value + self.captured_init_value
    }

    /// Fraction of coalescing-candidate mints that resolved to a slot, in
    /// `[0, 1]`. `None` when there were no candidate sites at all.
    pub fn slot_fraction(&self) -> Option<f64> {
        let total = self.coalesced() + self.value_resolved();
        (total != 0).then(|| self.coalesced() as f64 / total as f64)
    }
}

const EMPTY: RcCoalesceStats = RcCoalesceStats {
    return_mint_slot: 0,
    return_mint_value: 0,
    reassign_store_slot: 0,
    reassign_store_value: 0,
    captured_init_slot: 0,
    captured_init_value: 0,
    self_edges_eliminated: 0,
};

thread_local! {
    static STATS: Cell<RcCoalesceStats> = const { Cell::new(EMPTY) };
}

fn bump(f: impl FnOnce(&mut RcCoalesceStats)) {
    STATS.with(|s| {
        let mut v = s.get();
        f(&mut v);
        s.set(v);
    });
}

/// Record `lower_return`'s mint decision (`coalesced` ⇒ slot-resolved).
pub(super) fn record_return_mint(coalesced: bool) {
    bump(|s| {
        if coalesced {
            s.return_mint_slot += 1;
        } else {
            s.return_mint_value += 1;
        }
    });
}

/// Record the reassign incref-on-store's decision (`coalesced` ⇒ slot-resolved).
pub(super) fn record_reassign_store(coalesced: bool) {
    bump(|s| {
        if coalesced {
            s.reassign_store_slot += 1;
        } else {
            s.reassign_store_value += 1;
        }
    });
}

/// Record `store_captured_cell_init`'s init-drop decision (`coalesced` ⇒
/// slot-resolved `DecrefRegion`).
pub(super) fn record_captured_init(coalesced: bool) {
    bump(|s| {
        if coalesced {
            s.captured_init_slot += 1;
        } else {
            s.captured_init_value += 1;
        }
    });
}

/// Record one transform-2 self-edge elimination.
pub(super) fn record_self_edge_eliminated() {
    bump(|s| s.self_edges_eliminated += 1);
}

/// Read the current thread's accumulated stats without clearing them.
pub fn snapshot() -> RcCoalesceStats {
    STATS.with(|s| s.get())
}

/// Clear the current thread's stats. Call before a measured compile.
pub fn reset() {
    STATS.with(|s| s.set(EMPTY));
}

/// Read and clear in one step, so the next measurement starts fresh.
pub fn take() -> RcCoalesceStats {
    STATS.with(|s| s.replace(EMPTY))
}
