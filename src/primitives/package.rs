// audited: 2026-09-23
//! What this build of Elle is: its version, its epoch, the cargo profile it
//! was compiled under, and the fingerprint of the binary itself.
//!
//! docs/test-store.md

use std::fs::File;
use std::hash::Hasher;
use std::io::{self, Read};
use std::sync::OnceLock;

use crate::epoch::CURRENT_EPOCH;
use crate::primitives::def::RegionEffect;
use crate::value::fiber::{SignalBits, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;

/// Get the current package version
pub(crate) fn prim_package_version(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    (SIG_OK, ctx.string(env!("CARGO_PKG_VERSION")))
}

/// Get the current language epoch.
/// With 0 args: returns the current epoch.
/// With 1 arg: identity (the compiler strips `(elle/epoch N)` before this runs).
pub(crate) fn prim_epoch(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if args.is_empty() {
        (SIG_OK, Value::int(CURRENT_EPOCH as i64))
    } else {
        (SIG_OK, args[0])
    }
}

/// The cargo profile this binary was compiled under.
///
/// `debug_assertions` is the profile's own switch: cargo sets it for `dev`
/// and clears it for `release`, and module resolution already reads it to
/// pick `target/debug` over `target/release`. No other reading of the profile
/// survives into the running binary.
pub(crate) fn prim_build_profile(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    (SIG_OK, ctx.string(profile))
}

/// The path of the running binary, or nil when the OS will not say.
///
/// A program that wants to run Elle again has to name the Elle it is already
/// running, and nothing else in the process can: `(sys/argv)` carries the
/// source it was pointed at, and a subcommand replaces even that. Resolving
/// `elle` off `PATH` would answer with a different build, so the test runner
/// spawns this (docs/test-cli.md).
pub(crate) fn prim_executable(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    match std::env::current_exe() {
        Ok(path) => {
            let owned = path.to_string_lossy().into_owned();
            (SIG_OK, ctx.string(&owned))
        }
        Err(_) => (SIG_OK, Value::NIL),
    }
}

// ── The boot fingerprint ───────────────────────────────────────────────
//
// A verdict is a function of the form, the binary, the boot sources, and the
// run configuration. Nothing in a process could name the binary, so a recorded
// result belonged to a commit and never to a build, and a cached green could
// not say which executable earned it (docs/test-store.md).
//
// The three sources a boot compiles — core.lisp, prelude.lisp and stdlib.lisp
// — are built into the executable, so hashing its bytes covers the binary and
// the boot sources in one number. Hashing the embedded copies as well would
// add nothing: they are bytes this already read.

/// Bytes hashed per pass over the executable.
const CHUNK: usize = 64 * 1024;

/// This executable's boot fingerprint, or `None` when the OS will not name the
/// running binary or it cannot be read.
///
/// Computed once: the answer is a property of one file on disk, and a process
/// that asked twice would otherwise read it twice.
pub fn boot_fingerprint() -> Option<u64> {
    static PRINT: OnceLock<Option<u64>> = OnceLock::new();
    *PRINT.get_or_init(compute_fingerprint)
}

fn compute_fingerprint() -> Option<u64> {
    let path = std::env::current_exe().ok()?;
    let mut file = File::open(path).ok()?;
    digest(&mut file).ok()
}

/// The digest of every byte `image` yields.
///
/// The hash is the one already in the tree for a disposable local key: 64-bit,
/// fast enough to pass over a debug binary without anybody noticing, and
/// stable for a build. It is not a content address, and a fingerprint that
/// travels between machines wants a real digest first.
fn digest(image: &mut impl Read) -> io::Result<u64> {
    let mut hasher = rustc_hash::FxHasher::default();
    let mut buf = vec![0u8; CHUNK];
    loop {
        let have = fill(image, &mut buf)?;
        hasher.write(&buf[..have]);
        if have < buf.len() {
            return Ok(hasher.finish());
        }
    }
}

/// Fill `buf` from `image` and answer how many bytes arrived, short of the
/// buffer's length only at the end of the stream.
///
/// The trap: a hasher takes each slice as it comes, so two readings that split
/// one stream differently answer differently. A `Read` may return short
/// whenever it likes, so filling the buffer first is what makes the digest a
/// function of the bytes rather than of how they arrived.
fn fill(image: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut have = 0;
    while have < buf.len() {
        match image.read(&mut buf[have..])? {
            0 => break,
            n => have += n,
        }
    }
    Ok(have)
}

/// The boot fingerprint of this binary, or nil where the OS will not name it.
///
/// The hash itself, as an integer: what reads a fingerprint compares, groups
/// and joins it, and rendering it would cost an allocation to hand back a
/// number the caller has to parse again.
pub(crate) fn prim_boot_fingerprint(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    match boot_fingerprint() {
        Some(print) => (SIG_OK, Value::int(print as i64)),
        None => (SIG_OK, Value::NIL),
    }
}

/// Get package information
pub(crate) fn prim_package_info(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    let items = vec![
        ctx.string("Elle"),
        ctx.string(env!("CARGO_PKG_VERSION")),
        ctx.string("A Lisp interpreter with bytecode compilation"),
    ];
    (SIG_OK, ctx.list(items))
}

// Declarative primitive definitions for package operations
primitive! {
    "elle/version" => prim_package_version {
        doc: "Get the current package version",
        category: "elle",
        example: "(elle/version)",
        aliases: &["pkg/version", "package-version"],
        effect: RegionEffect::Fresh,
    }
    "elle/epoch" => prim_epoch {
        arity: Arity::Range(0, 1),
        doc: "Return the current language epoch. With 1 arg, returns the arg (compile-time declaration form).",
        params: &["n"],
        category: "elle",
        example: "(int? (elle/epoch))  # => true",
        effect: RegionEffect::PassThrough,
    }
    "elle/build-profile" => prim_build_profile {
        doc: "Return the cargo profile this binary was compiled under: \"debug\" or \"release\".",
        category: "elle",
        example: "(elle/build-profile)",
        effect: RegionEffect::Fresh,
    }
    "elle/executable" => prim_executable {
        doc: "Return the path of the running elle binary, or nil when the OS will not say.",
        category: "elle",
        example: "(elle/executable)",
        effect: RegionEffect::Fresh,
    }
    "elle/boot-fingerprint" => prim_boot_fingerprint {
        doc: "Return this binary's boot fingerprint — a 64-bit hash over the running executable, which carries the sources it boots from — or nil when the OS will not name it.",
        category: "elle",
        example: "(elle/boot-fingerprint)",
        // The hash is an integer and nil is an immediate, so the result never
        // reaches the heap (docs/impl/region/effects.md).
        effect: RegionEffect::Immediate,
    }
    "elle/info" => prim_package_info {
        doc: "Get package information (name, version, description)",
        category: "elle",
        example: "(elle/info)",
        aliases: &["pkg/info", "package-info"],
        effect: RegionEffect::Fresh,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest_of(bytes: &[u8]) -> u64 {
        digest(&mut &bytes[..]).expect("a slice always reads")
    }

    #[test]
    fn one_image_digests_the_same_every_time() {
        assert_eq!(digest_of(b"elle"), digest_of(b"elle"));
    }

    #[test]
    fn a_changed_byte_moves_the_digest() {
        assert_ne!(
            digest_of(b"elle"),
            digest_of(b"ellf"),
            "a binary differing by one byte is a different binary"
        );
    }

    /// The counter-factual for the pass over the file: a digest built from the
    /// first chunk alone agrees on every pair of images that share an opening,
    /// which is every pair of builds of one program.
    #[test]
    fn a_change_past_the_first_chunk_moves_the_digest() {
        let mut tail_differs = vec![b'x'; CHUNK * 2 + 7];
        let same_opening = tail_differs.clone();
        *tail_differs.last_mut().expect("a non-empty image") = b'y';
        assert_ne!(digest_of(&tail_differs), digest_of(&same_opening));
    }

    /// Whatever binary runs this test can be named and read, so it has a
    /// fingerprint — and asking twice answers once.
    #[test]
    fn the_running_binary_has_one_fingerprint() {
        assert_eq!(boot_fingerprint(), boot_fingerprint());
        assert!(boot_fingerprint().is_some(), "the test binary names itself");
    }
}
