// audited: 2026-11-20
//! A file operation on the ring: the opcodes each takes, and its first SQE.
//!
//! docs/impl/io/platforms.md
//! docs/impl/io/cancel.md

use super::*;
use crate::io::pending::stage::FileStage;
use crate::io::request::FileOp;
use io_uring::{opcode, squeue, types};

mod after;
pub(super) use after::after;

/// What a stat asks `statx(2)` for, which is what std's metadata asks for.
const STAT_MASK: u32 = libc::STATX_BASIC_STATS | libc::STATX_BTIME;

/// The mode a new directory takes before the umask, as `create_dir` gives it.
const DIR_MODE: libc::mode_t = 0o777;

/// The opcodes `op` takes on the ring, or `None` for a walk, which has none.
pub(super) fn opcodes(op: &FileOp) -> Option<&'static [u8]> {
    Some(match op {
        FileOp::Stat { .. } | FileOp::Size { .. } | FileOp::Probe { .. } => &[opcode::Statx::CODE],
        FileOp::Mkdir { parents: false, .. } | FileOp::MakeTempDir { .. } => {
            &[opcode::MkDirAt::CODE]
        }
        FileOp::Delete { .. }
        | FileOp::DeleteDir {
            recursive: false, ..
        } => &[opcode::UnlinkAt::CODE],
        FileOp::Rename { .. } => &[opcode::RenameAt::CODE],
        FileOp::Sync { .. } => &[
            opcode::OpenAt::CODE,
            opcode::Statx::CODE,
            opcode::Fsync::CODE,
        ],
        FileOp::List { .. }
        | FileOp::Mkdir { parents: true, .. }
        | FileOp::DeleteDir {
            recursive: true, ..
        }
        | FileOp::Canonicalize { .. } => return None,
    })
}

/// Submit the first SQE of `op`, which `Ring::takes`. Its paths go into
/// `buf_handle` with their NULs, where the kernel reads them until the CQE
/// arrives. Answers the stage the operation's entry keeps.
pub(crate) fn submit_uring_file(
    ring: &mut io_uring::IoUring,
    id: SubmissionId,
    op: &FileOp,
    buffer_pool: &mut BufferPool,
    buf_handle: BufferHandle,
) -> Result<FileStage, String> {
    let buf = buffer_pool.get_mut(buf_handle);
    let (stage, entry) = match op {
        FileOp::List { .. }
        | FileOp::Mkdir { parents: true, .. }
        | FileOp::DeleteDir {
            recursive: true, ..
        }
        | FileOp::Canonicalize { .. } => {
            return Err(format!(
                "{}: a walk has no ring form",
                op.kind().primitive()
            ))
        }
        FileOp::Stat { path, follow } => stat(buf, path, *follow),
        FileOp::Size { path } | FileOp::Probe { path, .. } => stat(buf, path, true),
        FileOp::Mkdir { path, .. } => (FileStage::Whole, mkdir(place(buf, &[path])[0])),
        FileOp::MakeTempDir { root } => {
            let stage = FileStage::temp_dir(root);
            let making = stage.making().expect("a temporary directory names its try");
            let at = place(buf, &[making]);
            (stage, mkdir(at[0]))
        }
        FileOp::Delete { path } => (FileStage::Whole, unlink(place(buf, &[path])[0], 0)),
        FileOp::DeleteDir { path, .. } => {
            let at = place(buf, &[path]);
            (FileStage::Whole, unlink(at[0], libc::AT_REMOVEDIR))
        }
        FileOp::Rename { from, to } => {
            let at = place(buf, &[from, to]);
            let cwd = types::Fd(libc::AT_FDCWD);
            let entry = opcode::RenameAt::new(cwd, at[0], cwd, at[1]).build();
            (FileStage::Whole, entry)
        }
        FileOp::Sync { path } => {
            // Non-blocking, as the pool's sync opens: a fifo opens at once.
            let flags = libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NONBLOCK;
            let at = place(buf, &[path]);
            let entry = opcode::OpenAt::new(types::Fd(libc::AT_FDCWD), at[0])
                .flags(flags)
                .build();
            (FileStage::sync(), entry)
        }
    };
    // SAFETY: the paths live in the pooled buffer, and a stat's record in the
    // stage, until the CQE arrives.
    unsafe { submit_linked(ring, id, entry.user_data(id.as_u64()), None) }?;
    Ok(stage)
}

/// The first SQE of a stat of `path`, through a final symlink when `follow`
/// holds, and the stage whose buffer the kernel writes the record into.
fn stat(buf: &mut Vec<u8>, path: &str, follow: bool) -> (FileStage, squeue::Entry) {
    let stage = FileStage::stat();
    let FileStage::Stat(record) = &stage else {
        unreachable!("a stat's stage holds its record");
    };
    let mut flags = libc::AT_STATX_SYNC_AS_STAT;
    if !follow {
        flags |= libc::AT_SYMLINK_NOFOLLOW;
    }
    let at = place(buf, &[path]);
    let entry = opcode::Statx::new(types::Fd(libc::AT_FDCWD), at[0], record.as_mut_ptr().cast())
        .flags(flags)
        .mask(STAT_MASK)
        .build();
    (stage, entry)
}

/// The SQE that makes the directory at `path`.
fn mkdir(path: *const libc::c_char) -> squeue::Entry {
    opcode::MkDirAt::new(types::Fd(libc::AT_FDCWD), path)
        .mode(DIR_MODE)
        .build()
}

/// The SQE that removes what `path` names, with `unlinkat(2)`'s `flags`.
fn unlink(path: *const libc::c_char, flags: i32) -> squeue::Entry {
    opcode::UnlinkAt::new(types::Fd(libc::AT_FDCWD), path)
        .flags(flags)
        .build()
}

/// Lay `paths` into `buf`, each with its NUL, and answer where each starts.
/// The buffer is not written again until the CQE arrives, so the pointers
/// hold for the SQE that names them.
fn place(buf: &mut Vec<u8>, paths: &[&str]) -> Vec<*const libc::c_char> {
    buf.clear();
    let mut starts = Vec::with_capacity(paths.len());
    for path in paths {
        starts.push(buf.len());
        buf.extend_from_slice(path.as_bytes());
        buf.push(0);
    }
    starts
        .into_iter()
        .map(|s| buf[s..].as_ptr().cast())
        .collect()
}
