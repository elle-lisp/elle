// audited: 2026-11-20
//! What each CQE of a file operation on the ring leads to: its answer, or its next step.
//!
//! docs/impl/io/platforms.md
//! docs/impl/io/cancel.md

use super::super::resubmit::Next;
use super::*;
use crate::io::pending::stage::{next_temp_name, StatxBuf};
use crate::io::request::{FileKind, FileType, PathTest, StatRecord};
use crate::io::threadpool::{out_of_names, syncable, TEMP_ATTEMPTS};

/// What a file operation does after a CQE carrying `result_code`: answer, or
/// go on to its next SQE with what it now owns in its stage.
pub(in crate::io::uring) fn after(
    id: SubmissionId,
    op: &mut PendingOp,
    result_code: i32,
    buffer_pool: &mut BufferPool,
) -> Next {
    let PendingOp::File {
        kind,
        stage,
        buffer_handle,
        ..
    } = op
    else {
        unreachable!("after: not a file operation");
    };
    let bh = *buffer_handle;
    let done = |result_code, data| Next::Complete {
        result_code,
        data,
        buf_handle: Some(bh),
    };
    let again = |entry: squeue::Entry| Next::Again(entry.user_data(id.as_u64()));
    // Whatever the stage owned and does not carry on is let go when this
    // match ends: a stat's record once read, a sync's descriptor once answered.
    match std::mem::replace(stage, FileStage::Whole) {
        FileStage::Whole => done(result_code, Vec::new()),
        FileStage::Stat(buf) => {
            // SAFETY: this CQE answers the SQE that named the buffer.
            let sx = unsafe { buf.take() };
            match (*kind, result_code < 0) {
                // A probe answers false for a path it cannot reach.
                (FileKind::Probe(_), true) => done(0, Vec::new()),
                (_, true) => done(result_code, Vec::new()),
                (kind, false) => {
                    let (code, data) = answer_stat(kind, &StatRecord::from_statx(&sx));
                    done(code, data)
                }
            }
        }
        FileStage::TempDir {
            root,
            making,
            tried,
        } => match result_code {
            0.. => done(0, making.into_bytes()),
            code if code != -libc::EEXIST => done(code, Vec::new()),
            _ if tried >= TEMP_ATTEMPTS => {
                let (code, why) = out_of_names();
                done(code, why)
            }
            _ => {
                let making = next_temp_name(&root);
                let entry = mkdir(place(buffer_pool.get_mut(bh), &[&making])[0]);
                *stage = FileStage::TempDir {
                    root,
                    making,
                    tried: tried + 1,
                };
                again(entry)
            }
        },
        FileStage::Sync { fd: None, .. } if result_code >= 0 => {
            // SAFETY: the open answered this descriptor, and nothing else
            // holds it.
            let fd = unsafe { OwnedFd::from_raw_fd(result_code) };
            let stat = StatxBuf::new();
            let flags = libc::AT_EMPTY_PATH | libc::AT_STATX_SYNC_AS_STAT;
            let entry = opcode::Statx::new(
                types::Fd(fd.as_raw_fd()),
                c"".as_ptr(),
                stat.as_mut_ptr().cast(),
            )
            .flags(flags)
            .mask(libc::STATX_TYPE)
            .build();
            *stage = FileStage::Sync {
                fd: Some(fd),
                stat: Some(stat),
            };
            again(entry)
        }
        FileStage::Sync {
            fd: Some(fd),
            stat: Some(buf),
        } if result_code >= 0 => {
            // SAFETY: this CQE answers the SQE that named the buffer.
            let sx = unsafe { buf.take() };
            if !syncable(libc::mode_t::from(sx.stx_mode)) {
                return done(-libc::EINVAL, Vec::new());
            }
            let entry = opcode::Fsync::new(types::Fd(fd.as_raw_fd())).build();
            *stage = FileStage::Sync {
                fd: Some(fd),
                stat: None,
            };
            again(entry)
        }
        FileStage::Sync { stat, .. } => {
            if let Some(buf) = stat {
                // SAFETY: this CQE answers the SQE that named the buffer.
                unsafe { buf.free() };
            }
            done(result_code.min(0), Vec::new())
        }
    }
}

/// What a stat of `kind` answers for the record `rec`: the code and the bytes
/// the pool's worker sends for it.
fn answer_stat(kind: FileKind, rec: &StatRecord) -> (i32, Vec<u8>) {
    match kind {
        FileKind::Size => (0, rec.size.to_le_bytes().to_vec()),
        FileKind::Probe(test) => {
            let holds = match test {
                PathTest::Exists => true,
                PathTest::File => rec.file_type == FileType::File,
                PathTest::Dir => rec.file_type == FileType::Dir,
            };
            (i32::from(holds), Vec::new())
        }
        _ => (0, rec.encode()),
    }
}
