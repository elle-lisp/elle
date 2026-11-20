// audited: 2026-11-20
//! Resolving a path to an absolute one with no symlink in it, one component at a time, asking before each whether to stop.
//!
//! docs/io/files.md
//! docs/impl/io/cancel.md

use super::walk::{errno, report, step, Halt};
use std::ffi::OsStr;
use std::os::unix::ffi::{OsStrExt, OsStringExt};

/// How many symlinks one resolution follows before it fails with `ELOOP`: the
/// count the platform's own `realpath(3)` allows.
#[cfg(target_os = "macos")]
const MAX_LINKS: u32 = 32;
#[cfg(not(target_os = "macos"))]
const MAX_LINKS: u32 = 40;

/// `path` resolved through every symlink: 0 and the absolute path, as
/// `std::fs::canonicalize` answers. A path that resolves to bytes that are
/// not UTF-8 fails with `EINVAL`, since an Elle string cannot hold it.
pub(super) fn canonicalize(path: &str, stop: &mut dyn FnMut() -> bool) -> (i32, Vec<u8>) {
    report(resolve(path.as_bytes(), stop).and_then(|resolved| {
        String::from_utf8(resolved)
            .map(String::into_bytes)
            .map_err(|_| {
                Halt::Failed(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "the resolved path is not valid UTF-8",
                ))
            })
    }))
}

/// `path` with every `.`, `..` and symlink resolved, as `realpath(3)` resolves
/// it. A relative path starts from the working directory. A `..` applies to
/// what the path names on disk, so after a symlink it leaves the link's target.
///
/// Each component is one `lstat(2)`, and a symlink's target goes in front of
/// what is left to resolve. A component that is not a directory fails with
/// `ENOTDIR` when anything follows it, a slash included.
fn resolve(path: &[u8], stop: &mut dyn FnMut() -> bool) -> Result<Vec<u8>, Halt> {
    if path.is_empty() {
        return Err(errno(libc::ENOENT));
    }
    let mut resolved = if path[0] == b'/' {
        b"/".to_vec()
    } else {
        std::env::current_dir()?.into_os_string().into_vec()
    };
    let mut rest = path.to_vec();
    let mut links = 0;
    while let Some(start) = rest.iter().position(|&b| b != b'/') {
        let end = rest[start..]
            .iter()
            .position(|&b| b == b'/')
            .map_or(rest.len(), |at| start + at);
        let name = rest[start..end].to_vec();
        let follows = end < rest.len();
        rest.drain(..end);
        match name.as_slice() {
            b"." => continue,
            b".." => {
                let parent = resolved.iter().rposition(|&b| b == b'/').unwrap_or(0);
                resolved.truncate(parent.max(1));
                continue;
            }
            _ => {}
        }
        step(stop)?;
        let mut next = resolved.clone();
        if next.last() != Some(&b'/') {
            next.push(b'/');
        }
        next.extend_from_slice(&name);
        let meta = std::fs::symlink_metadata(OsStr::from_bytes(&next))?;
        if meta.file_type().is_symlink() {
            links += 1;
            if links > MAX_LINKS {
                return Err(errno(libc::ELOOP));
            }
            let mut target = std::fs::read_link(OsStr::from_bytes(&next))?
                .into_os_string()
                .into_vec();
            if target.first() == Some(&b'/') {
                resolved = b"/".to_vec();
            }
            target.extend_from_slice(&rest);
            rest = target;
            continue;
        }
        if follows && !meta.is_dir() {
            return Err(errno(libc::ENOTDIR));
        }
        resolved = next;
    }
    Ok(resolved)
}
