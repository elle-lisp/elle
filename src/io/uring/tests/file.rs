// audited: 2026-11-20
//! A file operation's steps on the ring: a mktempdir's next name after `EEXIST`, and a sync's descriptor.
//!
//! docs/impl/io/platforms.md
//! docs/impl/io/cancel.md

use super::super::resubmit::{next, Next};
use super::*;
use crate::io::pending::FileStage;
use crate::io::request::FileKind;
use crate::scratch::ScratchDir;
use std::os::unix::io::{AsRawFd, FromRawFd, OwnedFd, RawFd};

/// What `next` decides for `op` after a CQE carrying `result_code`.
fn after(op: &mut PendingOp, result_code: i32, pool: &mut BufferPool) -> Next {
    let gen = crate::config::get().unicode_generation();
    next(
        SubmissionId::from_raw(1),
        op,
        result_code,
        pool,
        &mut HashMap::new(),
        gen,
    )
}

/// The name a ring mktempdir is making now.
fn making(op: &PendingOp) -> String {
    match op {
        PendingOp::File { stage, .. } => stage.making().expect("a name").to_string(),
        _ => panic!("not a file operation"),
    }
}

/// A ring mktempdir takes the next name after `EEXIST`, as the pool's does,
/// and gives up with the pool's error after as many names. Success answers
/// the name it made, and any other failure answers at once.
///
/// The counter-factual: a mktempdir that answered its first `EEXIST` would
/// fail wherever a dead process left a directory under a recycled pid. One
/// that never gave up would resubmit forever under a root it cannot write.
#[test]
fn a_ring_mktempdir_takes_the_next_name_until_it_runs_out() {
    let root = ScratchDir::new("ring-tempdir");
    let mut pool = BufferPool::new();
    let fresh = |pool: &mut BufferPool| {
        PendingOp::file(
            FileKind::MakeTempDir,
            root.as_str(),
            pool.alloc(0),
            FileStage::temp_dir(root.as_str()),
        )
    };

    let mut op = fresh(&mut pool);
    let mut names = vec![making(&op)];
    let mut tries = 1;
    let answer = loop {
        match after(&mut op, -libc::EEXIST, &mut pool) {
            Next::Again(_) => {
                tries += 1;
                names.push(making(&op));
            }
            Next::Complete {
                result_code, data, ..
            } => break (result_code, data),
        }
    };
    assert_eq!(tries, crate::io::threadpool::TEMP_ATTEMPTS, "names tried");
    names.sort();
    names.dedup();
    assert_eq!(names.len() as u64, tries, "each try takes a new name");
    assert!(
        names.iter().all(|n| n.starts_with(root.as_str())),
        "every name is under the root"
    );
    assert_eq!(answer.0, -libc::EEXIST);
    assert_eq!(
        String::from_utf8(answer.1).unwrap(),
        format!("no free name after {tries} attempts")
    );

    let mut op = fresh(&mut pool);
    let name = making(&op);
    match after(&mut op, 0, &mut pool) {
        Next::Complete {
            result_code, data, ..
        } => {
            assert_eq!(result_code, 0);
            assert_eq!(data, name.into_bytes(), "the answer is the name made");
        }
        Next::Again(_) => panic!("a made directory answers"),
    }

    let mut op = fresh(&mut pool);
    match after(&mut op, -libc::EACCES, &mut pool) {
        Next::Complete { result_code, .. } => assert_eq!(result_code, -libc::EACCES),
        Next::Again(_) => panic!("only EEXIST takes another name"),
    }
}

/// A pipe whose read end is non-blocking, and its write end.
fn pipe() -> (OwnedFd, RawFd) {
    let mut fds = [0; 2];
    let rc = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) };
    assert_eq!(rc, 0, "pipe2");
    // SAFETY: the kernel just handed this descriptor to this test.
    (unsafe { OwnedFd::from_raw_fd(fds[0]) }, fds[1])
}

/// Whether every write end of the pipe `r` reads is closed.
fn writer_closed(r: &OwnedFd) -> bool {
    let mut byte = 0u8;
    unsafe { libc::read(r.as_raw_fd(), (&mut byte as *mut u8).cast(), 1) == 0 }
}

/// A ring sync holds the descriptor its open answered across its later steps,
/// and lets go of it when it answers, or when a retire ends it between steps.
///
/// A pipe's write end stands in for the file's descriptor, so the test sees
/// each close from the read end.
///
/// The counter-factual: a sync that closed the descriptor after its open would
/// stat and fsync a number another thread may already hold. One that never
/// closed it would leak a descriptor per `file/sync`.
#[test]
fn a_ring_sync_holds_its_descriptor_until_it_answers() {
    let mut pool = BufferPool::new();
    let sync = |pool: &mut BufferPool| {
        PendingOp::file(FileKind::Sync, "synced", pool.alloc(0), FileStage::sync())
    };

    let (r, w) = pipe();
    let mut op = sync(&mut pool);
    assert!(
        matches!(after(&mut op, w, &mut pool), Next::Again(_)),
        "an open that answered goes on to its stat"
    );
    assert!(!writer_closed(&r), "the sync holds its descriptor");
    match after(&mut op, -libc::EACCES, &mut pool) {
        Next::Complete { result_code, .. } => assert_eq!(result_code, -libc::EACCES),
        Next::Again(_) => panic!("a failed stat answers"),
    }
    assert!(writer_closed(&r), "the answer let the descriptor go");
    drop(op);

    let (r, w) = pipe();
    let mut op = sync(&mut pool);
    assert!(matches!(after(&mut op, w, &mut pool), Next::Again(_)));
    op.retire(-libc::ECANCELED, &[], &mut pool);
    assert!(writer_closed(&r), "a retire between steps closes it");

    let mut op = sync(&mut pool);
    match after(&mut op, -libc::ENOENT, &mut pool) {
        Next::Complete { result_code, .. } => assert_eq!(result_code, -libc::ENOENT),
        Next::Again(_) => panic!("a failed open answers"),
    }
}
