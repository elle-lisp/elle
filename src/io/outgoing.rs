//! audited: 2026-11-12
//! `Outgoing`, a write's bytes: one payload or the parts of a gathered write,
//! read where they lie or copied once.
//!
//! docs/impl/io/bytes.md

use crate::value::Value;
use smallvec::{smallvec, SmallVec};

/// A write's bytes part by part, each where it lies. One payload is one part.
pub(crate) type Parts<'a> = SmallVec<[&'a [u8]; 4]>;

/// The bytes a write sends, and the one question every backend asks of them:
/// do they lie in region pages, or must they be copied once?
///
/// A backend never asks which primitive made the write. It asks `in_region`,
/// and copies with `copy_into` when the answer is no.
#[derive(Debug, Clone, Copy)]
pub enum Outgoing {
    /// One payload, from `port/write`.
    One(Value),
    /// The parts of a gathered write, from `port/write-many`: an array, an
    /// `@array` or a list of strings and bytes.
    Many(Value),
}

impl Outgoing {
    /// The bytes of one payload.
    pub(crate) fn one(data: Value) -> Outgoing {
        Outgoing::One(data)
    }

    /// The bytes of `parts`, which the primitive has checked are strings and
    /// bytes in an array, an `@array` or a list.
    pub(crate) fn many(parts: Value) -> Outgoing {
        Outgoing::Many(parts)
    }

    /// The heap value these bytes are reached through. The pending entry holds
    /// it for as long as the write is in flight (docs/impl/io/inflight.md), and
    /// a container's region holds the regions of its parts.
    pub(crate) fn value(&self) -> Value {
        match self {
            Outgoing::One(data) | Outgoing::Many(data) => *data,
        }
    }

    /// The bytes as they lie in region pages, part by part, which a write
    /// hands the kernel by address. `None` for bytes that must be copied
    /// first: a mutable part, or the parts of an `@array`, which another fiber
    /// can replace while the write waits.
    pub(crate) fn in_region(&self) -> Option<Parts<'_>> {
        match self {
            Outgoing::One(data) => region_bytes(*data).map(|bytes| smallvec![bytes]),
            Outgoing::Many(parts) => immutable_parts(*parts)?.map(region_bytes).collect(),
        }
    }

    /// Every byte the write sends.
    pub(crate) fn len(&self) -> usize {
        match self.in_region() {
            Some(parts) => parts.iter().map(|p| p.len()).sum(),
            None => self.copied().len(),
        }
    }

    /// Copy every byte into `out`, replacing what it held.
    pub(crate) fn copy_into(&self, out: &mut Vec<u8>) {
        out.clear();
        match self {
            Outgoing::One(data) => append_payload(*data, out),
            Outgoing::Many(parts) => match parts.array_mut_read() {
                Some(read) => append_parts(read.iter().copied(), out),
                None => append_parts(immutable_parts(*parts).into_iter().flatten(), out),
            },
        }
    }

    /// Every byte, copied once into a buffer of its own.
    pub(crate) fn copied(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.copy_into(&mut out);
        out
    }
}

/// The parts of an immutable array or of a list, in order. `None` for any
/// other container.
fn immutable_parts(parts: Value) -> Option<impl Iterator<Item = Value> + Clone> {
    let array = parts.is_array();
    if !array && !parts.is_list() {
        return None;
    }
    let mut at = 0;
    let mut rest = parts;
    Some(std::iter::from_fn(move || {
        if array {
            at += 1;
            return parts.as_array()?.get(at - 1).copied();
        }
        let pair = rest.as_pair()?;
        let first = pair.first;
        rest = pair.rest;
        Some(first)
    }))
}

/// The bytes an immutable string or bytes holds in its region.
///
/// The borrow is not tied to `data`: a region's bytes stay where they are for
/// as long as the region lives, and the write's hold keeps it alive.
fn region_bytes<'a>(data: Value) -> Option<&'a [u8]> {
    let bytes = data
        .as_bytes()
        .or_else(|| data.as_str().map(str::as_bytes))?;
    // SAFETY: see above; region pages never move.
    Some(unsafe { std::slice::from_raw_parts(bytes.as_ptr(), bytes.len()) })
}

/// Append every part to `out`, which grows once, to their total.
fn append_parts(parts: impl Iterator<Item = Value> + Clone, out: &mut Vec<u8>) {
    out.reserve(parts.clone().map(payload_size).sum());
    parts.for_each(|part| append_payload(part, out));
}

/// The bytes a string or bytes appends. 0 for a value written as its display
/// text, which `out` grows to hold as it is written.
fn payload_size(data: Value) -> usize {
    region_bytes(data)
        .map(<[u8]>::len)
        .or_else(|| data.bytes_mut_read().map(|b| b.len()))
        .or_else(|| data.string_mut_read().map(|s| s.len()))
        .unwrap_or(0)
}

/// Append a payload's bytes to `out`. A mutable `@string` or `@bytes` is
/// copied because another fiber can grow it and move its bytes; any other
/// value is written as its display text.
fn append_payload(data: Value, out: &mut Vec<u8>) {
    if let Some(bytes) = region_bytes(data) {
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

/// The most iovecs one gathered write hands the kernel: `IOV_MAX` on Linux and
/// macOS.
pub(crate) const IOV_MAX: usize = 1024;

/// The iovecs that send `parts` from byte `done` on: none for an empty part,
/// and at most `IOV_MAX` of them carrying at most `most` bytes.
pub(crate) fn tail_iovecs<'a>(
    parts: &'a [&'a [u8]],
    done: usize,
    most: usize,
) -> impl Iterator<Item = libc::iovec> + 'a {
    parts
        .iter()
        .scan((done, most), |(skip, room), part| {
            if *room == 0 {
                return None;
            }
            if *skip >= part.len() {
                *skip -= part.len();
                return Some(None);
            }
            let tail = &part[*skip..];
            *skip = 0;
            let take = tail.len().min(*room);
            *room -= take;
            Some(Some(libc::iovec {
                iov_base: tail.as_ptr() as *mut libc::c_void,
                iov_len: take,
            }))
        })
        .flatten()
        .take(IOV_MAX)
}

/// What a pool worker writes from.
pub(crate) enum Payload {
    /// Each part's own region bytes, by address. The pending entry holds their
    /// regions until the completion arrives, and a backend's teardown waits for
    /// the worker, so the bytes outlive every write that reads them.
    Addressed(SmallVec<[(*const u8, usize); 4]>),
    /// A copy the worker owns.
    Owned(Vec<u8>),
}

// SAFETY: an `Addressed` payload is read by one worker while the entry that
// filed it holds its regions; see the variant.
unsafe impl Send for Payload {}

impl Payload {
    /// The parts' region bytes, by address.
    pub(crate) fn addressed(parts: &[&[u8]]) -> Payload {
        Payload::Addressed(parts.iter().map(|p| (p.as_ptr(), p.len())).collect())
    }

    /// Every part of the payload, in order.
    pub(crate) fn parts(&self) -> Parts<'_> {
        match self {
            // SAFETY: see `Payload::Addressed`.
            Payload::Addressed(spans) => spans
                .iter()
                .map(|&(ptr, len)| unsafe { std::slice::from_raw_parts(ptr, len) })
                .collect(),
            Payload::Owned(v) => smallvec![v.as_slice()],
        }
    }
}

#[cfg(test)]
mod tests;
