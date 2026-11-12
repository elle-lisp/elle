//! audited: 2026-11-11
//! `Outgoing`, a write's bytes: one payload or the parts of a gathered write,
//! read where they lie or copied once.
//!
//! docs/impl/io-bytes.md

use crate::value::Value;

/// The bytes a write sends, and the one question every backend asks of them:
/// do they lie in region pages, or must they be copied once?
///
/// A backend never asks which primitive made the write. It asks
/// `in_region`, and copies with
/// `copy_into` when the answer is no.
#[derive(Debug, Clone, Copy)]
pub enum Outgoing {
    /// One payload, from `port/write`.
    One(Value),
}

impl Outgoing {
    /// The bytes of one payload.
    pub(crate) fn one(data: Value) -> Outgoing {
        Outgoing::One(data)
    }

    /// The heap value these bytes are reached through. The pending entry holds
    /// it for as long as the write is in flight (docs/impl/io-inflight.md).
    pub(crate) fn value(&self) -> Value {
        match self {
            Outgoing::One(data) => *data,
        }
    }

    /// The bytes as they lie in region pages, which a write hands the kernel
    /// by address. `None` for bytes that must be copied first.
    pub(crate) fn in_region(&self) -> Option<&[u8]> {
        match self {
            Outgoing::One(data) => payload_in_region(data),
        }
    }

    /// Copy every byte into `out`, replacing what it held.
    pub(crate) fn copy_into(&self, out: &mut Vec<u8>) {
        match self {
            Outgoing::One(data) => copy_payload_into(data, out),
        }
    }

    /// Every byte, copied once into a buffer of its own.
    pub(crate) fn copied(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.copy_into(&mut out);
        out
    }
}

/// The bytes an immutable string or bytes payload holds in its region.
fn payload_in_region(data: &Value) -> Option<&[u8]> {
    data.as_bytes().or_else(|| data.as_str().map(str::as_bytes))
}

/// Copy a payload's bytes into `out`, replacing what it held. A mutable
/// `@string` or `@bytes` is copied because another fiber can grow it and move
/// its bytes; any other value is written as its display text.
fn copy_payload_into(data: &Value, out: &mut Vec<u8>) {
    out.clear();
    if let Some(bytes) = payload_in_region(data) {
        out.extend_from_slice(bytes);
    } else if let Some(b) = data.bytes_mut_read() {
        out.extend_from_slice(&b);
    } else if let Some(b) = data.string_mut_read() {
        out.extend_from_slice(&b);
    } else {
        use std::io::Write;
        let _ = write!(out, "{}", data);
    }
}

/// What a pool worker writes from.
pub(crate) enum Payload {
    /// The payload's own region bytes. The pending entry holds their region
    /// until the completion arrives, and a backend's teardown waits for the
    /// worker, so the bytes outlive every write that reads them.
    Addressed { ptr: *const u8, len: usize },
    /// A copy the worker owns.
    Owned(Vec<u8>),
}

// SAFETY: an `Addressed` payload is read by one worker while the entry that
// filed it holds its region; see the variant.
unsafe impl Send for Payload {}

impl Payload {
    /// The payload's region bytes, by address.
    pub(crate) fn addressed(bytes: &[u8]) -> Payload {
        Payload::Addressed {
            ptr: bytes.as_ptr(),
            len: bytes.len(),
        }
    }

    /// Every byte of the payload.
    pub(crate) fn bytes(&self) -> &[u8] {
        match self {
            // SAFETY: see `Payload::Addressed`.
            Payload::Addressed { ptr, len } => unsafe { std::slice::from_raw_parts(*ptr, *len) },
            Payload::Owned(v) => v,
        }
    }
}
