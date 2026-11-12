// audited: 2026-11-11
//! What the prim_id registry answers: every def resolves back to itself, and a reader never waits on a writer.
//!
//! docs/impl/values.md

use super::{lock_writers, prim_def, prim_id_of, prim_table_snapshot};
use crate::primitives::def::PrimitiveDef;
use crate::primitives::registration::canonical_defs;

/// A def no table holds, so the registry appends it. It lives as long as the
/// registry that names it, which is the whole process.
fn fresh_def() -> &'static PrimitiveDef {
    Box::leak(Box::new(PrimitiveDef {
        name: "registry-test-def",
        ..PrimitiveDef::DEFAULT
    }))
}

fn address(def: &PrimitiveDef) -> usize {
    def as *const PrimitiveDef as usize
}

/// The canonical defs hold the first ids, in table order. The stdlib cache
/// keys a stored `prim_id` on that order alone, so a seed that numbered them
/// any other way would hand a cached native-fn another def.
#[test]
fn a_canonical_def_holds_its_table_position() {
    for (position, def) in canonical_defs().enumerate() {
        let id = prim_id_of(def);
        assert_eq!(id as usize, position, "{} is out of table order", def.name);
        let found = prim_def(id).unwrap_or_else(|| panic!("{}'s id names nothing", def.name));
        assert_eq!(address(found), address(def), "id {id} names another def");
    }
}

/// Defs appended from many threads at once each get an id of their own, and
/// each id resolves to the def that got it. The 2,000 defs they add are several
/// times the canonical count, so the registry grows while the threads read it.
#[test]
fn defs_appended_from_many_threads_resolve_to_themselves() {
    let workers: Vec<_> = (0..8)
        .map(|_| {
            std::thread::spawn(|| {
                (0..250)
                    .map(|_| {
                        let def = fresh_def();
                        let id = prim_id_of(def);
                        assert_eq!(prim_id_of(def), id, "a def asked twice gets two ids");
                        (id, address(def))
                    })
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    let mut seen = std::collections::HashSet::new();
    for worker in workers {
        for (id, def) in worker.join().expect("an appending thread") {
            assert!(seen.insert(id), "id {id} went to two defs");
            let found = prim_def(id).unwrap_or_else(|| panic!("appended id {id} names nothing"));
            assert_eq!(address(found), def, "appended id {id} names another def");
        }
    }
}

#[test]
fn an_id_never_handed_out_names_nothing() {
    assert!(prim_def(u32::MAX).is_none());
}

#[test]
fn the_snapshot_agrees_with_prim_def() {
    let snapshot = prim_table_snapshot();
    assert!(snapshot.len() >= canonical_defs().count());
    for (id, def) in snapshot.iter().enumerate() {
        let found = prim_def(id as u32).unwrap_or_else(|| panic!("snapshot id {id} names nothing"));
        assert_eq!(
            address(found),
            address(def),
            "snapshot id {id} names another def"
        );
    }
}

/// `prim_def` answers while another thread holds the lock `prim_id_of` takes.
///
/// The counter-factual is a `prim_def` that reads behind that lock. The reader
/// then blocks until the guard drops, and the receive times out.
///
/// The trap: the guard drops before the assertion. A test that panics while it
/// holds the guard poisons the registry, and every other test in the process
/// that names a native-fn then fails on the poison instead of on its own claim.
#[test]
fn a_reader_does_not_wait_for_the_writer_lock() {
    let first = canonical_defs().next().expect("a canonical def");
    let guard = lock_writers();
    let (answer_tx, answer_rx) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let _ = answer_tx.send(prim_def(0).map(address));
    });
    let answer = answer_rx.recv_timeout(std::time::Duration::from_secs(10));
    drop(guard);
    reader.join().expect("the reader thread");
    assert_eq!(
        answer,
        Ok(Some(address(first))),
        "prim_def waited for the writer lock"
    );
}
