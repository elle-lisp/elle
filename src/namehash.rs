//! The 64-bit name hash that gives symbols and keywords their identity.
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

/// FNV-1a over the name's UTF-8 bytes.
///
/// `const fn` so a well-known name's hash can be a constant. Uses `while`
/// rather than `for`: `for` desugars to `IntoIterator::into_iter`, which is not
/// const-compatible.
pub const fn name_hash(name: &str) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000000000001b;
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
