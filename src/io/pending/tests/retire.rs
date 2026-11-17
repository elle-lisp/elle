// audited: 2026-11-17
//! What a retired entry gives back on disk: the directory a cancelled mktempdir made, and nothing else.
//!
//! docs/impl/io/cancel.md

use super::*;
use crate::io::request::FileKind;
use crate::scratch::ScratchDir;

/// A file operation's entry of `kind`, under `path`.
fn file_op(pool: &mut BufferPool, kind: FileKind, path: &str) -> PendingOp {
    PendingOp::File {
        kind,
        path: path.to_string(),
        buffer_handle: pool.alloc(0),
    }
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
