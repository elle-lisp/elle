// audited: 2026-10-22
//! The `@string` and `@bytes` store: an extent of bytes in the container's region, edited by one splice.
//!
//! docs/impl/mutable.md
//!
//! An `@string`'s bytes are UTF-8, and the caller of a splice keeps them so.

use super::extent::{Extent, ExtentRead};

/// The contents of an `@string` or of `@bytes`.
pub type ByteStore = Extent<u8>;

/// A read of a byte store. A splice panics while one is live.
pub type ByteRead<'a> = ExtentRead<'a, u8>;
