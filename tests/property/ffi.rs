// Property tests for the FFI module.
//
// Tests cover: pointer tagged-union invariants, marshal range checking,
// memory read-write roundtrips, TypeDesc size/align consistency,
// string marshalling edge cases, and struct/array marshalling roundtrips.
//
// This file is a COORDINATOR: it holds the shared imports and the
// `prim_ffi_*` test shims, then pulls in the themed test bodies via
// `include!`. Each included module sees these helpers through `super::*`.

use elle::ffi::marshal::MarshalledArg;
use elle::ffi::types::TypeDesc;
use elle::primitives::ctx::with_test_ctx;
use elle::primitives::memory::{
    prim_ffi_align as raw_ffi_align, prim_ffi_free as raw_ffi_free,
    prim_ffi_malloc as raw_ffi_malloc, prim_ffi_read as raw_ffi_read,
    prim_ffi_size as raw_ffi_size, prim_ffi_write as raw_ffi_write,
};
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::Value;
use proptest::prelude::*;

// The FFI primitives take a `NativeCtx` (docs/impl/region-ctx.md); these
// bare-signature shims route each call through the `with_test_ctx` seam so the
// property-test bodies below stay unchanged.
fn prim_ffi_malloc(args: &[Value]) -> (SignalBits, Value) {
    with_test_ctx(|ctx| raw_ffi_malloc(ctx, args))
}
fn prim_ffi_free(args: &[Value]) -> (SignalBits, Value) {
    with_test_ctx(|ctx| raw_ffi_free(ctx, args))
}
fn prim_ffi_read(args: &[Value]) -> (SignalBits, Value) {
    with_test_ctx(|ctx| raw_ffi_read(ctx, args))
}
fn prim_ffi_write(args: &[Value]) -> (SignalBits, Value) {
    with_test_ctx(|ctx| raw_ffi_write(ctx, args))
}
fn prim_ffi_size(args: &[Value]) -> (SignalBits, Value) {
    with_test_ctx(|ctx| raw_ffi_size(ctx, args))
}
fn prim_ffi_align(args: &[Value]) -> (SignalBits, Value) {
    with_test_ctx(|ctx| raw_ffi_align(ctx, args))
}

use crate::property::strategies::{arb_flat_struct, arb_primitive_type, arb_struct_and_values};

// Themed test bodies. Sections A-B cover pointer/marshal invariants; C-G cover
// memory roundtrips and struct write/read; H-J cover layout, array, and
// FFIType value properties.
mod pointer {
    include!("ffi/pointer.rs");
}
mod memory {
    include!("ffi/memory.rs");
}
mod layout {
    include!("ffi/layout.rs");
}
