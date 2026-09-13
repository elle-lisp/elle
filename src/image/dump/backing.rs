// audited: 2026-09-13
//! One slice's backing bytes, and how they reach the pages section.
//!
//! docs/impl/image.md
//!
//! emit.rs records where each backing lands; these write the bytes. A raw
//! backing copies as it stands; the three assembled forms — struct entries,
//! code payloads, slice headers — go through the layout probes, so no
//! construction temporary's padding reaches the artifact.

use std::mem::size_of;

use crate::value::closure::CodePayload;
use crate::value::region_slice::RegionSlice;
use crate::value::{TableKey, Value};

use super::super::layout;

/// One slice's backing bytes, and how they reach the file.
pub(super) enum Backing {
    /// Bytes copied as they stand: a string or byte payload, a `Value` slice,
    /// a scope set — none of them has padding to leak.
    Raw { rel: u64, src: usize, len: usize },
    /// Struct entries, each assembled from probed extents. An entry's key is
    /// an enum whose padding a copy would carry into the artifact
    /// (docs/impl/image.md § Dumping).
    Entries { rel: u64, src: usize, count: usize },
    /// One code payload, assembled from probed offsets: twelve slice headers,
    /// an arity through its own probe, and a scalar tail
    /// (docs/impl/image.md § Dumping).
    Payload { rel: u64, src: usize },
    /// Slice headers end to end — a payload's file names or `&named` keys.
    /// Each element's `ptr` and `len` are copied and the padding after the
    /// length stays zero.
    SliceHeaders { rel: u64, src: usize, count: usize },
}

impl Backing {
    /// `count` elements of `T` starting at `src`, landing at image offset
    /// `rel`, copied as bytes.
    pub(super) fn raw<T>(rel: u64, src: usize, count: usize) -> Backing {
        Backing::Raw {
            rel,
            src,
            len: count * size_of::<T>(),
        }
    }

    pub(super) fn entries(rel: u64, src: usize, count: usize) -> Backing {
        Backing::Entries { rel, src, count }
    }

    pub(super) fn payload(rel: u64, p: &CodePayload) -> Backing {
        Backing::Payload {
            rel,
            src: p as *const CodePayload as usize,
        }
    }

    pub(super) fn slice_headers(rel: u64, src: usize, count: usize) -> Backing {
        Backing::SliceHeaders { rel, src, count }
    }

    pub(super) fn write(&self, pages: &mut [u8]) {
        match *self {
            Backing::Raw { rel, src, len } => {
                let bytes = unsafe { std::slice::from_raw_parts(src as *const u8, len) };
                pages[rel as usize..rel as usize + len].copy_from_slice(bytes);
            }
            Backing::Entries { rel, src, count } => {
                let entries =
                    unsafe { std::slice::from_raw_parts(src as *const (TableKey, Value), count) };
                let stride = size_of::<(TableKey, Value)>();
                for (i, e) in entries.iter().enumerate() {
                    let at = rel as usize + i * stride;
                    layout::write_canonical_entry(e, &mut pages[at..at + stride]);
                }
            }
            Backing::Payload { rel, src } => {
                let p = unsafe { &*(src as *const CodePayload) };
                let size = size_of::<CodePayload>();
                layout::write_canonical_payload(p, &mut pages[rel as usize..rel as usize + size]);
            }
            Backing::SliceHeaders { rel, src, count } => {
                let (ptr_at, len_at, len_size) = RegionSlice::<u8>::header_layout();
                let stride = size_of::<RegionSlice<u8>>();
                let headers =
                    unsafe { std::slice::from_raw_parts(src as *const RegionSlice<u8>, count) };
                for (i, h) in headers.iter().enumerate() {
                    let at = rel as usize + i * stride;
                    let bytes =
                        unsafe { std::slice::from_raw_parts(h as *const _ as *const u8, stride) };
                    pages[at + ptr_at..at + ptr_at + size_of::<*const u8>()]
                        .copy_from_slice(&bytes[ptr_at..ptr_at + size_of::<*const u8>()]);
                    pages[at + len_at..at + len_at + len_size]
                        .copy_from_slice(&bytes[len_at..len_at + len_size]);
                }
            }
        }
    }
}
