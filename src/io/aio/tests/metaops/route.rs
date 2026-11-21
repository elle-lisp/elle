// audited: 2026-11-20
//! Which platform runs a file operation on the ring's host: the kernel's own opcode, or a pool worker.
//!
//! docs/impl/io/platforms.md

use super::super::pathops::answer_of;
use super::*;
use crate::io::request::{FileOp, PathTest};
use crate::scratch::ScratchDir;

/// One of each operation that is not a walk, in `dir`, each of which answers
/// without an error.
fn calls(dir: &ScratchDir) -> Vec<FileOp> {
    let file = dir.join_str("file");
    std::fs::write(&file, "hello").unwrap();
    std::fs::create_dir(dir.join_str("empty")).unwrap();
    std::fs::write(dir.join_str("doomed"), "").unwrap();
    let p = |s: &str| s.to_string();
    vec![
        FileOp::Stat {
            path: p(&file),
            follow: true,
        },
        FileOp::Stat {
            path: p(&file),
            follow: false,
        },
        FileOp::Size { path: p(&file) },
        FileOp::Probe {
            path: p(&file),
            test: PathTest::File,
        },
        FileOp::Sync { path: p(&file) },
        FileOp::Mkdir {
            path: dir.join_str("made"),
            parents: false,
        },
        FileOp::Rename {
            from: p(&file),
            to: dir.join_str("moved"),
        },
        FileOp::Delete {
            path: dir.join_str("doomed"),
        },
        FileOp::DeleteDir {
            path: dir.join_str("empty"),
            recursive: false,
        },
        FileOp::MakeTempDir {
            root: dir.as_str().to_string(),
        },
    ]
}

/// The four walks, in `dir`, each of which answers without an error.
fn walks(dir: &ScratchDir) -> Vec<FileOp> {
    std::fs::create_dir(dir.join_str("tree")).unwrap();
    vec![
        FileOp::List {
            path: dir.as_str().to_string(),
        },
        FileOp::Canonicalize {
            path: dir.as_str().to_string(),
        },
        FileOp::Mkdir {
            path: dir.join_str("deep/er"),
            parents: true,
        },
        FileOp::DeleteDir {
            path: dir.join_str("tree"),
            recursive: true,
        },
    ]
}

/// Submit `op` to `backend`, and answer how many pool workers it took before
/// any completion was reaped. Then reap its answer, which must not fail.
fn workers_taken(backend: &AsyncBackend, op: FileOp) -> usize {
    let kind = op.kind();
    let req = IoRequest::unbounded(IoOp::File(op), Value::NIL);
    let id = backend
        .submit(&req, crate::io::pending::Submitter::for_test())
        .unwrap();
    let taken = backend.workers();
    let answer = answer_of(backend, id);
    assert!(answer.result.is_ok(), "{kind:?}: {:?}", answer.result);
    answer.discard();
    taken
}

/// On the ring, every file operation but a walk runs as the kernel's own
/// opcode and takes no pool worker. A walk takes one on either platform.
///
/// The counter-factual: an operation the ring sends to the pool counts a
/// worker until its completion is reaped, so the count right after the
/// submission says which platform runs it.
#[test]
fn a_file_operation_on_the_ring_takes_no_worker_unless_it_walks() {
    crate::value::arena::with_test_region(|| {
        let backend = AsyncBackend::new().unwrap();
        if !backend.is_uring() {
            return;
        }
        let dir = ScratchDir::new("meta-ring");
        for op in calls(&dir) {
            let kind = op.kind();
            assert_eq!(workers_taken(&backend, op), 0, "{kind:?} on the ring");
        }
        for op in walks(&dir) {
            let kind = op.kind();
            assert_eq!(workers_taken(&backend, op), 1, "{kind:?} on a worker");
        }
    });
}

/// An operation that needs an opcode the kernel lacks runs on the pool, and
/// answers as it would on the ring. Without `IORING_OP_STATX`, a stat, a size,
/// a probe and a sync each take a worker, and a delete stays on the ring.
///
/// The counter-factual: a ring that submitted an opcode the kernel lacks would
/// answer `EINVAL` for every stat on an older kernel.
#[test]
fn an_operation_whose_opcode_the_kernel_lacks_runs_on_the_pool() {
    crate::value::arena::with_test_region(|| {
        let backend = AsyncBackend::new().unwrap();
        if !backend.is_uring() {
            return;
        }
        backend.refuse_ring_opcode(io_uring::opcode::Statx::CODE);
        let dir = ScratchDir::new("meta-refused");
        for op in calls(&dir) {
            let kind = op.kind();
            let needs_statx = matches!(
                op,
                FileOp::Stat { .. }
                    | FileOp::Size { .. }
                    | FileOp::Probe { .. }
                    | FileOp::Sync { .. }
            );
            assert_eq!(
                workers_taken(&backend, op),
                usize::from(needs_statx),
                "{kind:?}"
            );
        }
    });
}
