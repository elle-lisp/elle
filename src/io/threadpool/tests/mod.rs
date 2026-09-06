// audited: 2026-09-05
// src/io/AGENTS.md
// Unit tests for the thread-pool backend, one module per concern.
//
//   - `hub`:      CompletionHub in_flight accounting (the one-channel invariant)
//   - `net`:      what a connect makes of a refusal, and the platform
//                 behavior the paced retry rests on
//   - `opbound`:  the per-operation timeout bound, on a pipe
//   - `pool`:     worker reuse: the crew's handover, growth, and retirement
//   - `process`:  ProcessWait completion encoding
//   - `openfile`: Open op fd/errno results
//   - `signals`:  forked signalfd/kqueue read + close-time drain regressions
//   - `stdin`:    stdin worker shutdown (idle and mid-read)
mod hub;
mod net;
mod opbound;
mod openfile;
mod pool;
mod process;
mod signals;
mod stdin;

use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A unique scratch path under the platform temp root.
///
/// The root honors `TMPDIR` and is never a hardcoded `/tmp` or `/dev/shm`;
/// the latter is a Linux tmpfs that does not exist on macOS or the BSDs. The
/// pid and the counter keep concurrent test binaries, and two tests within
/// one binary, off each other's files. Callers create the file and remove it
/// before returning.
fn file_path(tag: &str) -> String {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir()
        .join(format!("elle-tp-{}-{}-{}", tag, std::process::id(), n))
        .to_str()
        .unwrap()
        .to_string()
}
