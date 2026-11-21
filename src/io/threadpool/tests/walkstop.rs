// audited: 2026-11-20
//! A file walk a stop ends: before its first step through the hub, and between two steps when its check says so.
//!
//! docs/impl/io/cancel.md

use super::super::*;
use super::super::{canon, walk};
use crate::io::request::FileOp;
use crate::scratch::ScratchDir;

/// Submit `op` with its stop already written, and answer the code it reports.
fn stopped_first(id: u64, op: FileOp) -> i32 {
    let mut hub = CompletionHub::new();
    let id = SubmissionId::from_raw(id);
    let bounds = hub.bounds(id, Bound::NONE);
    hub.stop(id);
    hub.submit(id, PoolOp::File(op), bounds).unwrap();
    let completions = hub
        .wait_pool(Some(5000))
        .expect("a stopped walk must report back");
    assert_eq!(completions.len(), 1, "one walk, one completion");
    completions[0].result_code
}

/// The names in `dir`, sorted.
fn names(dir: &str) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

/// A directory of `n` empty files, `f0` to `f{n-1}`.
fn flat(dir: &ScratchDir, name: &str, n: usize) -> String {
    let top = dir.join_str(name);
    std::fs::create_dir(&top).unwrap();
    for i in 0..n {
        std::fs::write(format!("{top}/f{i}"), "").unwrap();
    }
    top
}

/// A check that answers "go on" `n` times, then "stop".
fn stop_after(n: usize) -> impl FnMut() -> bool {
    let mut asked = 0;
    move || {
        asked += 1;
        asked > n
    }
}

/// The counter-factual: a walk that never looks at its stop pipe runs to its
/// end and answers 0, with the tree gone and the directories made.
#[test]
fn a_stop_written_first_ends_each_walk_before_its_first_step() {
    let dir = ScratchDir::new("tp-walk-first");
    let tree = flat(&dir, "tree", 3);
    std::fs::create_dir(format!("{tree}/sub")).unwrap();
    std::fs::write(format!("{tree}/sub/leaf"), "").unwrap();
    let before = names(&tree);

    let delete = FileOp::DeleteDir {
        path: tree.clone(),
        recursive: true,
    };
    assert_eq!(stopped_first(30, delete), -libc::ECANCELED, "a delete");
    assert_eq!(names(&tree), before, "the delete removed nothing");
    assert_eq!(names(&format!("{tree}/sub")), vec!["leaf"]);

    let deep = dir.join_str("a/b/c");
    let mkdir = FileOp::Mkdir {
        path: deep,
        parents: true,
    };
    assert_eq!(stopped_first(31, mkdir), -libc::ECANCELED, "a mkdir-all");
    assert!(
        !std::path::Path::new(&dir.join_str("a")).exists(),
        "it made nothing"
    );

    let list = FileOp::List { path: tree.clone() };
    assert_eq!(stopped_first(32, list), -libc::ECANCELED, "a list");
    let canon = FileOp::Canonicalize {
        path: format!("{tree}/sub/leaf"),
    };
    assert_eq!(stopped_first(33, canon), -libc::ECANCELED, "a canonicalize");
}

/// Run `walk` once per stop point, the check saying "stop" at its first ask,
/// then its second, and so on, until the walk runs to its end. Answer how many
/// runs it stopped in. `fresh` makes what each run walks.
///
/// A walk that checks once per step stops in at least one run per step, so
/// the count says the check comes between steps rather than once at the start.
fn stopped_runs(
    mut fresh: impl FnMut() -> String,
    mut walk: impl FnMut(&str, &mut dyn FnMut() -> bool) -> (i32, Vec<u8>),
    mut after_stop: impl FnMut(&str),
) -> usize {
    for n in 0..10_000 {
        let path = fresh();
        let (code, why) = walk(&path, &mut stop_after(n));
        if code >= 0 {
            return n;
        }
        assert_eq!(
            code,
            -libc::ECANCELED,
            "stopped at ask {n}: {}",
            String::from_utf8_lossy(&why)
        );
        after_stop(&path);
    }
    panic!("the walk never ran to its end");
}

#[test]
fn a_delete_stops_between_two_entries_and_leaves_the_rest() {
    let dir = ScratchDir::new("tp-walk-delete");
    let mut run = 0;
    let mut partial = 0;
    let stops = stopped_runs(
        || {
            run += 1;
            flat(&dir, &format!("t{run}"), 20)
        },
        walk::delete_tree,
        |tree| {
            let left = names(tree).len();
            assert!(left > 0 || std::path::Path::new(tree).exists());
            if (1..20).contains(&left) {
                partial += 1;
            }
        },
    );
    assert!(stops >= 20, "a check per entry: {stops} stop points");
    assert!(
        partial > 0,
        "a stopped delete keeps the entries it has not reached"
    );
}

#[test]
fn a_mkdir_all_stops_between_two_components_and_keeps_what_it_made() {
    let dir = ScratchDir::new("tp-walk-mkdir");
    let mut run = 0;
    let mut kept = 0;
    let stops = stopped_runs(
        || {
            run += 1;
            dir.join_str(&format!("r{run}/a/b/c/d/e"))
        },
        walk::make_dirs,
        |deep| {
            assert!(!std::path::Path::new(deep).exists(), "{deep} is not made");
            let root = deep.trim_end_matches("/a/b/c/d/e");
            if std::path::Path::new(root).is_dir() {
                kept += 1;
            }
        },
    );
    assert!(stops >= 6, "a check per component: {stops} stop points");
    assert!(
        kept > 0,
        "a stopped mkdir-all keeps the directories it made"
    );
}

#[test]
fn a_list_stops_between_two_entries() {
    let dir = ScratchDir::new("tp-walk-list");
    let tree = flat(&dir, "tree", 20);
    let stops = stopped_runs(|| tree.clone(), walk::list, |_| {});
    assert!(stops >= 20, "a check per entry: {stops} stop points");
}

#[test]
fn a_canonicalize_stops_between_two_components() {
    let dir = ScratchDir::new("tp-walk-canon");
    std::fs::create_dir_all(dir.join_str("a/b/c/d/e")).unwrap();
    let path = dir.join_str("a/b/c/d/e");
    let stops = stopped_runs(|| path.clone(), canon::canonicalize, |_| {});
    assert!(stops >= 5, "a check per component: {stops} stop points");
}
