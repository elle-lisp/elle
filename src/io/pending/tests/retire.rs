// audited: 2026-11-20
//! What a retired entry gives back: the directory a cancelled mktempdir made, and the descriptor a ring's sync opened.
//!
//! docs/impl/io/cancel.md

use super::*;
use crate::io::request::FileKind;
use crate::scratch::ScratchDir;

/// A file operation's entry of `kind`, under `path`, that a pool worker runs.
fn file_op(pool: &mut BufferPool, kind: FileKind, path: &str) -> PendingOp {
    PendingOp::file(kind, path, pool.alloc(0), FileStage::Whole)
}

/// A retired mktempdir removes the directory its worker reported making,
/// because nobody else learns that name. Only a mktempdir that succeeded made
/// one: a failure's data is the error's text, and another operation's
/// directory is one its caller named.
#[test]
fn retiring_a_mktempdir_removes_the_directory_it_made_and_nothing_else() {
    let mut pool = BufferPool::new();
    let root = ScratchDir::new("retire");
    let named = root.join_str("named");
    let made = root.join_str("made");
    std::fs::create_dir(&named).unwrap();
    std::fs::create_dir(&made).unwrap();

    let mkdir = FileKind::Mkdir { parents: false };
    file_op(&mut pool, mkdir, &named).retire(0, named.as_bytes(), &mut pool);
    file_op(&mut pool, FileKind::MakeTempDir, root.as_str()).retire(
        -libc::EEXIST,
        named.as_bytes(),
        &mut pool,
    );
    assert!(
        std::path::Path::new(&named).is_dir(),
        "a mkdir or a failed mktempdir removes nothing"
    );

    file_op(&mut pool, FileKind::MakeTempDir, root.as_str()).retire(0, made.as_bytes(), &mut pool);
    assert!(
        !std::path::Path::new(&made).exists(),
        "the directory nobody else can name is gone"
    );
}

/// On the ring a retired mktempdir learns its name from its entry, because a
/// cancelled CQE carries no data. It removes the name only when the kernel's
/// `mkdirat` made it.
///
/// The counter-factual: a retire that read the name from the completion's
/// bytes would leave the directory in the temp root. One that removed the name
/// after `EEXIST` would remove a directory another process holds.
#[cfg(target_os = "linux")]
#[test]
fn retiring_a_ring_mktempdir_removes_the_name_it_made_and_no_other() {
    let mut pool = BufferPool::new();
    let root = ScratchDir::new("retire-ring");

    let stage = FileStage::temp_dir(root.as_str());
    let held = stage.making().expect("a name to make").to_string();
    std::fs::create_dir(&held).unwrap();
    PendingOp::file(FileKind::MakeTempDir, root.as_str(), pool.alloc(0), stage).retire(
        -libc::EEXIST,
        &[],
        &mut pool,
    );
    assert!(
        std::path::Path::new(&held).is_dir(),
        "a name the kernel found taken belongs to whoever took it"
    );

    let stage = FileStage::temp_dir(root.as_str());
    let made = stage.making().expect("a name to make").to_string();
    assert_ne!(made, held, "each stage takes a name of its own");
    std::fs::create_dir(&made).unwrap();
    PendingOp::file(FileKind::MakeTempDir, root.as_str(), pool.alloc(0), stage).retire(
        0,
        &[],
        &mut pool,
    );
    assert!(
        !std::path::Path::new(&made).exists(),
        "the directory the kernel made is gone"
    );
}

/// A pipe whose read end is non-blocking, so asking it whether every write
/// end is closed answers at once.
#[cfg(target_os = "linux")]
fn pipe() -> (std::os::unix::io::OwnedFd, std::os::unix::io::RawFd) {
    use std::os::unix::io::FromRawFd;
    let mut fds = [0; 2];
    let rc = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) };
    assert_eq!(rc, 0, "pipe2");
    // SAFETY: the kernel just handed this descriptor to this test.
    (
        unsafe { std::os::unix::io::OwnedFd::from_raw_fd(fds[0]) },
        fds[1],
    )
}

/// Whether every write end of the pipe `r` reads is closed: a read then
/// answers end of file rather than `EAGAIN`.
#[cfg(target_os = "linux")]
fn writer_closed(r: &std::os::unix::io::OwnedFd) -> bool {
    use std::os::unix::io::AsRawFd;
    let mut byte = 0u8;
    unsafe { libc::read(r.as_raw_fd(), (&mut byte as *mut u8).cast(), 1) == 0 }
}

/// A sync on the ring whose open answered owns that descriptor, and a retire
/// closes it, because no completion will.
///
/// A pipe's write end stands in for the file, so the test can see the close
/// from the read end without guessing at descriptor numbers another thread may
/// reuse.
#[cfg(target_os = "linux")]
#[test]
fn retiring_a_ring_sync_closes_the_descriptor_its_open_answered() {
    let mut pool = BufferPool::new();
    let (r, w) = pipe();
    assert!(!writer_closed(&r), "the write end is open");
    PendingOp::file(FileKind::Sync, "synced", pool.alloc(0), FileStage::sync()).retire(
        w,
        &[],
        &mut pool,
    );
    assert!(
        writer_closed(&r),
        "the retire closed what the open answered"
    );
}
