// audited: 2026-09-30
//! How a heap's regions end: the reclamation counters the `arena/*` gauges read.
//!
//! docs/impl/region/diagnostics.md

use super::*;

/// Every reclamation counter of one store, read at once. Each starts at 0 when
/// the store is made and never goes down, and a teardown moves none of them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ReclaimCounters {
    /// Regions freed, by a count reaching zero, an owner's drop, or a group free.
    pub region_frees: u64,
    /// The pages those regions held.
    pub page_frees: u64,
    /// The objects those regions held.
    pub object_frees: u64,
    /// The freed regions that held one page or none.
    pub one_page_frees: u64,
    /// The freed regions that held no object.
    pub empty_frees: u64,
    /// The freed regions that held one object.
    pub one_object_frees: u64,
    /// The freed regions that held two to four objects.
    pub few_object_frees: u64,
    /// The freed regions that held five objects or more.
    pub many_object_frees: u64,
    /// Counted regions made members of an owner's subtree.
    pub adopts: u64,
    /// Adoptions into an owner that held no object at the adopt.
    pub adopts_into_empty: u64,
    /// Owned regions freed by their owner's drop.
    pub owned_frees: u64,
    /// The pages those owned regions held.
    pub owned_free_pages: u64,
    /// The objects those owned regions held.
    pub owned_free_objects: u64,
    /// The owned regions freed while they held one page or none.
    pub owned_one_page_frees: u64,
    /// Owned regions the drop-time rescue returned to counted.
    pub rescues: u64,
    /// Regions that outlived their owner's drop through a rescue: each rescued
    /// region and the owned subtree it keeps.
    pub rescue_survivors: u64,
    /// Owned regions a moves-out removal returned to counted.
    pub extracts: u64,
    /// Owned regions handed from one owner to another.
    pub reparents: u64,
}

impl ReclaimCounters {
    /// Apply one event to this store's counters, and under `--dump=stats` to
    /// the process's too. Every count goes through here, so the two cannot
    /// differ.
    fn count(&mut self, event: impl Fn(&mut ReclaimCounters)) {
        event(self);
        if crate::config::get().stats {
            process::count(event);
        }
    }

    /// Count one region the free path tears down, `pages` and `objects` being
    /// what it held and `owned` whether its owner's drop is what freed it.
    pub(super) fn count_free(&mut self, pages: u64, objects: u64, owned: bool) {
        self.count(|c| {
            c.region_frees += 1;
            c.page_frees += pages;
            c.object_frees += objects;
            if pages <= 1 {
                c.one_page_frees += 1;
            }
            match objects {
                0 => c.empty_frees += 1,
                1 => c.one_object_frees += 1,
                2..=4 => c.few_object_frees += 1,
                _ => c.many_object_frees += 1,
            }
            if owned {
                c.owned_frees += 1;
                c.owned_free_pages += pages;
                c.owned_free_objects += objects;
                if pages <= 1 {
                    c.owned_one_page_frees += 1;
                }
            }
        });
    }

    /// Count one adoption, `into_empty` when the owner held no object.
    pub(super) fn count_adopt(&mut self, into_empty: bool) {
        self.count(|c| {
            c.adopts += 1;
            if into_empty {
                c.adopts_into_empty += 1;
            }
        });
    }

    /// Count one owned region a moves-out removal returned to counted.
    pub(super) fn count_extract(&mut self) {
        self.count(|c| c.extracts += 1);
    }

    /// Count `n` owned regions handed from one owner to another.
    pub(super) fn count_reparents(&mut self, n: u64) {
        self.count(|c| c.reparents += n);
    }

    /// Count one drop's rescue: `rescued` regions returned to counted, and the
    /// `survivors` that outlived the drop with them.
    pub(super) fn count_rescue(&mut self, rescued: u64, survivors: u64) {
        self.count(|c| {
            c.rescues += rescued;
            c.rescue_survivors += survivors;
        });
    }

    /// Every counter under its gauge name, in the order diagnostics.md's table
    /// gives, then `owned`: the adoptions that have not ended.
    fn report(&self) -> [(&'static str, i128); 19] {
        let ended = self.owned_frees + self.rescues + self.extracts;
        [
            ("region-frees", self.region_frees.into()),
            ("page-frees", self.page_frees.into()),
            ("object-frees", self.object_frees.into()),
            ("one-page-frees", self.one_page_frees.into()),
            ("empty-frees", self.empty_frees.into()),
            ("one-object-frees", self.one_object_frees.into()),
            ("few-object-frees", self.few_object_frees.into()),
            ("many-object-frees", self.many_object_frees.into()),
            ("adopts", self.adopts.into()),
            ("adopts-into-empty", self.adopts_into_empty.into()),
            ("owned-frees", self.owned_frees.into()),
            ("owned-free-pages", self.owned_free_pages.into()),
            ("owned-free-objects", self.owned_free_objects.into()),
            ("owned-one-page-frees", self.owned_one_page_frees.into()),
            ("rescues", self.rescues.into()),
            ("rescue-survivors", self.rescue_survivors.into()),
            ("extracts", self.extracts.into()),
            ("reparents", self.reparents.into()),
            ("owned", i128::from(self.adopts) - i128::from(ended)),
        ]
    }
}

/// The counters summed over every store in the process, kept under
/// `--dump=stats` and printed at exit.
mod process {
    use super::ReclaimCounters;
    use std::sync::{LazyLock, Mutex, Once};

    static TOTALS: LazyLock<Mutex<ReclaimCounters>> = LazyLock::new(Mutex::default);

    pub(super) fn count(event: impl Fn(&mut ReclaimCounters)) {
        // Registered at the first count, so the report also prints when the
        // process ends through `os/exit`, which runs C atexit handlers and no
        // Rust destructor.
        static REGISTER: Once = Once::new();
        REGISTER.call_once(|| unsafe {
            extern "C" fn at_exit() {
                print();
            }
            libc::atexit(at_exit);
        });
        event(&mut TOTALS.lock().unwrap_or_else(|e| e.into_inner()));
    }

    fn print() {
        let totals = *TOTALS.lock().unwrap_or_else(|e| e.into_inner());
        for (name, n) in totals.report() {
            eprintln!("[stats] reclaim {name}={n}");
        }
    }
}

impl RegionStore {
    /// The store's reclamation counters.
    pub(crate) fn reclaim_counters(&self) -> ReclaimCounters {
        self.counters
    }

    /// The regions owned now: a reading, not a count. A scan of the table, so
    /// it answers from the forest itself rather than from the counters it is
    /// checked against.
    pub(crate) fn owned_count(&self) -> u64 {
        self.regions
            .iter()
            .flatten()
            .filter(|e| matches!(e.reclaim, Reclaim::Owned { .. }))
            .count() as u64
    }
}
