// audited: 2026-09-20
//! The two fields that may name a process-owned resource, and the three
//! answers each one crosses as.
//!
//! A field holds program data, a reconstruction the hydrating instance
//! answers for itself, or a value that fails the dump.
//!
//! docs/impl/image/sealing.md

use crate::hir::region::RuntimeRegion;
use crate::port::Port;
use crate::value::fiberheap::FiberHeap;
use crate::value::Value;

use super::super::format::{Ctor, Stdio};
use super::copy::{copy_value, Walk};
use super::ImageError;

/// What the copy of an object carries in a field that may name a
/// process-owned resource (docs/impl/image/sealing.md).
pub(super) enum Crossing {
    /// Nothing: the source field was nil.
    None,
    /// The hydrating instance's own value, named by a constructor. The copy
    /// carries nil until hydration fills the slot in.
    Reconstruct(Ctor),
    /// Program data, copied into the image like any other value.
    Carried(Value),
}

impl Crossing {
    /// What the copy is built with. A reconstructed slot is written at
    /// hydration, so the dump leaves it nil and the emitter zeroes it.
    pub(super) fn value(&self) -> Value {
        match self {
            Crossing::Carried(v) => *v,
            _ => Value::NIL,
        }
    }
}

/// Decide what a source object's `traits` field crosses as, copying a user
/// table into `region` on the way.
///
/// The identity test runs over the whole default table rather than the
/// object's own tag: one traitset serves several tags, and `with-traits` can
/// attach any of them to any value.
pub(super) fn copy_traits(
    heap: &mut FiberHeap,
    region: RuntimeRegion,
    traits: Value,
    walk: &mut Walk,
) -> Result<Crossing, ImageError> {
    let Some(ptr) = traits.as_heap_ptr() else {
        return Ok(Crossing::None);
    };
    let default = heap
        .default_traits_table()
        .iter()
        .position(|t| t.as_heap_ptr() == Some(ptr));
    if let Some(i) = default {
        let tag = super::super::format::tag_from_u64(i as u64)?;
        return Ok(Crossing::Reconstruct(Ctor::DefaultTraits(tag)));
    }
    Ok(Crossing::Carried(copy_value(heap, region, traits, walk)?))
}

/// Decide what a `Parameter`'s `default` field crosses as.
///
/// An `External` is the one thing a default may hold that is not sealed data,
/// and among externals only a standard stream travels: the hydrating instance
/// opens its own. Everything else fails the dump here, naming what it met.
pub(super) fn copy_default(
    heap: &mut FiberHeap,
    region: RuntimeRegion,
    default: Value,
    walk: &mut Walk,
) -> Result<Crossing, ImageError> {
    let Some(type_name) = default.external_type_name() else {
        return Ok(Crossing::Carried(copy_value(heap, region, default, walk)?));
    };
    let kind = default.as_external::<Port>().map(|p| p.kind());
    match kind.and_then(Stdio::of) {
        Some(stream) => Ok(Crossing::Reconstruct(Ctor::StdioPort(stream))),
        None => Err(ImageError::Unsupported(match kind {
            Some(kind) => format!(
                "a {kind:?} port owns a descriptor this process opened, so no image carries it"
            ),
            None => format!(
                "a parameter's default holds a {type_name} external, which the image cannot rebuild"
            ),
        })),
    }
}
