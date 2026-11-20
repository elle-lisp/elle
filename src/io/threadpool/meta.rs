// audited: 2026-11-20
//! The operations a pool worker runs on a path's metadata in one call or one claim, and what each reports.
//!
//! docs/io/files.md
//! docs/impl/io/platforms.md

use super::file::failed;
use crate::io::request::{PathTest, StatRecord};
use std::sync::atomic::{AtomicU64, Ordering};

/// The number the next temporary directory of this process takes.
pub(super) static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// How many names a temporary directory tries before it gives up.
const TEMP_ATTEMPTS: u64 = 1024;

/// The metadata of `path`, through a final symlink when `follow` holds: 0 and
/// the encoded [`StatRecord`].
pub(super) fn stat(path: &str, follow: bool) -> (i32, Vec<u8>) {
    let meta = if follow {
        std::fs::metadata(path)
    } else {
        std::fs::symlink_metadata(path)
    };
    match meta {
        Ok(meta) => (0, StatRecord::from_metadata(&meta).encode()),
        Err(e) => failed(&e),
    }
}

/// The length of the file `path` names: 0 and eight little-endian bytes.
pub(super) fn size(path: &str) -> (i32, Vec<u8>) {
    match std::fs::metadata(path) {
        Ok(meta) => (0, meta.len().to_le_bytes().to_vec()),
        Err(e) => failed(&e),
    }
}

/// The path the `n`th temporary directory of this process takes under `root`.
pub(super) fn temp_candidate(root: &str, n: u64) -> String {
    crate::path::join(&[root, &format!("elle-{}-{}", std::process::id(), n)])
}

/// Make a directory under `root` with a name nothing holds, taking each
/// candidate's number from `counter`: 0 and the new path.
///
/// `create_dir` is the atomic claim. A name that exists already, which a
/// recycled pid meets when a dead process left one, takes the next number;
/// any other failure ends the search.
pub(super) fn make_temp_dir(root: &str, counter: &AtomicU64) -> (i32, Vec<u8>) {
    for _ in 0..TEMP_ATTEMPTS {
        let candidate = temp_candidate(root, counter.fetch_add(1, Ordering::Relaxed));
        match std::fs::create_dir(&candidate) {
            Ok(()) => return (0, candidate.into_bytes()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return failed(&e),
        }
    }
    let why = format!("no free name after {TEMP_ATTEMPTS} attempts");
    (-libc::EEXIST, why.into_bytes())
}

/// Whether `path` names what `test` asks for: 1 or 0, and never a failure. A
/// path the OS cannot reach answers 0.
pub(super) fn probe(path: &str, test: PathTest) -> (i32, Vec<u8>) {
    let holds = std::fs::metadata(path).is_ok_and(|meta| match test {
        PathTest::Exists => true,
        PathTest::File => meta.is_file(),
        PathTest::Dir => meta.is_dir(),
    });
    (i32::from(holds), Vec::new())
}
