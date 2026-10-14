// audited: 2026-10-14
//! The 64-bit name hash that gives symbols and keywords their identity.
//!
//! docs/impl/symbol.md
//!
//! Both are immediates in the 16-byte `Value`: the payload holds this hash, so
//! equality is `u64 == u64` — no string compare, no heap deref — and the same
//! name yields the same payload in every table, thread, process, and build.
//! Symbols reach it through [`crate::symbol`], keywords through
//! [`crate::value::keyword`].
//!
//! Two different names on one hash is a fatal, unrecoverable condition: both
//! registries panic rather than merge the names. See
//! [docs/impl/symbol.md](../../docs/impl/symbol.md) § "Collisions are fatal".

const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000000000001b;

/// FNV-1a over the name's UTF-8 bytes.
///
/// `const fn` so a well-known name's hash can be a constant. Uses `while`
/// rather than `for`: `for` desugars to `IntoIterator::into_iter`, which is not
/// const-compatible.
pub const fn name_hash(name: &str) -> u64 {
    let bytes = name.as_bytes();
    let mut hash = FNV_OFFSET;
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    hash
}

/// The same FNV-1a, fed in pieces, for a hash over more than one name. A unit
/// of expansion takes its tag from one ([`crate::syntax::UnitTag`]), and a
/// file the store keeps the digest of its compile inputs.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Fnv1a(u64);

impl Fnv1a {
    pub(crate) fn new() -> Self {
        Fnv1a(FNV_OFFSET)
    }

    pub(crate) fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= b as u64;
            self.0 = self.0.wrapping_mul(FNV_PRIME);
        }
    }

    /// Feed a count, as eight little-endian bytes.
    pub(crate) fn write_len(&mut self, len: usize) {
        self.write(&(len as u64).to_le_bytes());
    }

    /// Feed a string behind its length, so that two strings fed one after the
    /// other hash apart from any other split of the same bytes.
    pub(crate) fn write_str(&mut self, s: &str) {
        self.write_len(s.len());
        self.write(s.as_bytes());
    }

    pub(crate) fn finish(self) -> u64 {
        self.0
    }
}
