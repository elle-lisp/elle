//! audited: 2026-11-20
//! Operations on a path's metadata on both platforms: each answer, the error that names the path, and who runs it.
//!
//! docs/io/files.md
//! docs/impl/io/platforms.md

use super::pathops::{answer_of, platforms, run, run_on, Answer};
use super::*;
use crate::io::request::{FileOp, PathTest};
use crate::scratch::ScratchDir;

/// The names in `dir`, sorted.
fn names(dir: &ScratchDir) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir.as_str())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

fn stat(path: &str, follow: bool) -> std::collections::BTreeMap<&'static str, String> {
    match run(FileOp::Stat {
        path: path.to_string(),
        follow,
    }) {
        Ok(Answer::Struct(fields)) => fields,
        other => panic!("a stat answers a struct, not {other:?}"),
    }
}

/// The struct holds the keys `file/stat` has always held, each from the
/// path's own metadata.
#[test]
fn a_stat_answers_the_struct_of_what_the_path_names() {
    use std::os::unix::fs::MetadataExt;
    crate::value::arena::with_test_region(|| {
        let dir = ScratchDir::new("meta-stat");
        let file = dir.join_str("file");
        std::fs::write(&file, "hello").unwrap();
        let link = dir.join_str("link");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        let meta = std::fs::metadata(&file).unwrap();

        let fields = stat(&link, true);
        assert_eq!(fields.len(), 18, "every key is there: {fields:?}");
        let want = [
            ("file-type", "file".to_string()),
            ("size", "5".to_string()),
            ("is-file", "true".to_string()),
            ("is-dir", "false".to_string()),
            ("is-symlink", "false".to_string()),
            ("readonly", (meta.mode() & 0o222 == 0).to_string()),
            ("permissions", meta.mode().to_string()),
            ("inode", meta.ino().to_string()),
            ("nlinks", "1".to_string()),
        ];
        for (key, value) in want {
            assert_eq!(fields[key], value, ":{key}");
        }
        assert_ne!(fields["modified"], "nil", "a file has a modified time");

        let fields = stat(&link, false);
        assert_eq!(fields["file-type"], "symlink", "an lstat does not follow");
        assert_eq!(fields["is-symlink"], "true");
    });
}

/// Each operation answers what its primitive returns, on each platform. The
/// changes run in turn in a directory of each platform's own.
#[test]
fn each_answer_is_the_value_its_primitive_returns() {
    crate::value::arena::with_test_region(|| {
        for backend in platforms() {
            let run = |op| run_on(&backend, op);
            let dir = ScratchDir::new("meta-answers");
            let file = dir.join_str("file");
            std::fs::write(&file, "hello").unwrap();
            let path = |p: &str| p.to_string();
            let real = std::fs::canonicalize(dir.as_str()).unwrap();

            assert_eq!(run(FileOp::Size { path: path(&file) }), Ok(Answer::Int(5)));
            assert_eq!(
                run(FileOp::List {
                    path: dir.as_str().to_string()
                }),
                Ok(Answer::List(vec!["file".to_string()]))
            );
            let made = dir.join_str("a/b");
            let changes = [
                FileOp::Mkdir {
                    path: dir.join_str("a"),
                    parents: false,
                },
                FileOp::Mkdir {
                    path: path(&made),
                    parents: true,
                },
                FileOp::DeleteDir {
                    path: path(&made),
                    recursive: false,
                },
                FileOp::Rename {
                    from: path(&file),
                    to: dir.join_str("moved"),
                },
                FileOp::Delete {
                    path: dir.join_str("moved"),
                },
                FileOp::DeleteDir {
                    path: dir.join_str("a"),
                    recursive: true,
                },
            ];
            for op in changes {
                let kind = op.kind();
                assert_eq!(run(op), Ok(Answer::Bool(true)), "{kind:?}");
            }
            assert!(
                names(&dir).is_empty(),
                "each change landed: {:?}",
                names(&dir)
            );

            let probe = |test| {
                run(FileOp::Probe {
                    path: dir.as_str().to_string(),
                    test,
                })
            };
            assert_eq!(probe(PathTest::Exists), Ok(Answer::Bool(true)));
            assert_eq!(probe(PathTest::Dir), Ok(Answer::Bool(true)));
            assert_eq!(probe(PathTest::File), Ok(Answer::Bool(false)));
            assert_eq!(
                run(FileOp::Canonicalize {
                    path: format!("{}/.", dir.as_str())
                }),
                Ok(Answer::Text(real.to_str().unwrap().to_string()))
            );
            match run(FileOp::MakeTempDir {
                root: dir.as_str().to_string(),
            }) {
                Ok(Answer::Text(made)) => {
                    assert!(std::path::Path::new(&made).is_dir());
                    assert!(made.starts_with(dir.as_str()), "{made} is under its root");
                }
                other => panic!("a temporary directory answers its path, not {other:?}"),
            }
        }
    });
}

/// A failure is an `:io-error` that names the primitive and whose `:path`
/// names the path: the old path for a rename, the root for a temporary
/// directory. A probe never fails: a path it cannot reach answers false.
#[test]
fn a_failed_metadata_operation_names_its_path_and_a_probe_answers_false() {
    crate::value::arena::with_test_region(|| {
        let dir = ScratchDir::new("meta-failed");
        let missing = dir.join_str("missing");
        let m = || missing.clone();
        let ops = [
            FileOp::Stat {
                path: m(),
                follow: true,
            },
            FileOp::Stat {
                path: m(),
                follow: false,
            },
            FileOp::Size { path: m() },
            FileOp::List { path: m() },
            FileOp::Mkdir {
                path: format!("{missing}/x"),
                parents: false,
            },
            FileOp::Delete { path: m() },
            FileOp::DeleteDir {
                path: m(),
                recursive: false,
            },
            FileOp::DeleteDir {
                path: m(),
                recursive: true,
            },
            FileOp::Rename {
                from: m(),
                to: dir.join_str("to"),
            },
            FileOp::MakeTempDir { root: m() },
            FileOp::Canonicalize { path: m() },
        ];
        for op in ops {
            let kind = op.kind();
            let named = op.path().to_string();
            let (path, msg) = run(op).expect_err("a missing path fails");
            assert_eq!(path.as_deref(), Some(named.as_str()), "{kind:?}: :path");
            assert!(msg.contains("io-error"), "{kind:?}: an :io-error: {msg}");
            assert!(msg.contains(kind.primitive()), "{kind:?}: {msg}");
        }
        for test in [PathTest::Exists, PathTest::File, PathTest::Dir] {
            let probe = FileOp::Probe { path: m(), test };
            assert_eq!(run(probe), Ok(Answer::Bool(false)), "{test:?}");
        }
    });
}

/// A temporary directory whose fiber is gone is removed when its completion
/// is retired, because nobody else learns its name. On the pool the worker
/// reports the name, and on the ring the entry keeps it.
///
/// The cancel cannot stop a mktempdir already under way, so the directory
/// exists for a moment either way. The uncancelled run after it shows that
/// the root is where the platform makes one.
#[test]
fn a_cancelled_temp_dir_leaves_nothing_in_its_root() {
    crate::value::arena::with_test_region(|| {
        for backend in platforms() {
            let root = ScratchDir::new("meta-cancelled-tmp");
            let op = FileOp::MakeTempDir {
                root: root.as_str().to_string(),
            };
            let req = IoRequest::unbounded(IoOp::File(op.clone()), Value::NIL);
            let id = backend
                .submit(&req, crate::io::pending::Submitter::for_test())
                .unwrap();
            backend.cancel(id).unwrap();
            for _ in 0..100 {
                if !backend.has_pending() {
                    break;
                }
                Completion::discard_all(backend.wait(TICK).unwrap());
            }
            assert!(!backend.has_pending(), "the cancelled mktempdir retired");
            assert!(names(&root).is_empty(), "left behind: {:?}", names(&root));

            assert!(matches!(run_on(&backend, op), Ok(Answer::Text(_))));
            assert_eq!(names(&root).len(), 1, "an uncancelled one stays");
        }
    });
}

/// One of each operation that is not a walk, in `dir`, each of which answers
/// without an error.
#[cfg(target_os = "linux")]
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
#[cfg(target_os = "linux")]
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
#[cfg(target_os = "linux")]
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
#[cfg(target_os = "linux")]
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
#[cfg(target_os = "linux")]
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
