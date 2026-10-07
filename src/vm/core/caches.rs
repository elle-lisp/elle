// audited: 2026-10-07
//! The VM's caches keyed by a code object's bytecode address, and the pins that keep each key's code region alive.
//!
//! docs/impl/jit.md

use super::*;
#[cfg(feature = "jit")]
use std::sync::Arc;

#[cfg(feature = "jit")]
use crate::jit::JitCode;

/// A `jit_cache` entry: the compiled code plus the pin that keeps the keyed
/// bytecode alive (docs/impl/jit.md). The pin makes the raw-address key sound:
/// bytecode lives in a code object's payload, and the pin holds that payload's
/// code region, so the address cannot be reused by a different function while
/// this entry lives.
#[cfg(feature = "jit")]
pub struct JitCacheEntry {
    _pin: CodePin,
    pub code: Arc<JitCode>,
}

#[cfg(feature = "jit")]
impl JitCacheEntry {
    /// Build an entry held by `pin`, which names the entry's key.
    pub fn new(pin: CodePin, code: Arc<JitCode>) -> Self {
        JitCacheEntry { _pin: pin, code }
    }
}

/// The workgroup size a SPIR-V kernel is compiled for: a positive 32-bit
/// integer, written into the kernel's entry point (docs/impl/spirv.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WorkgroupSize(u32);

impl WorkgroupSize {
    /// The size `git`, `fn/git?`, `disgit` and `mlir/compile-spirv` take when
    /// none is given.
    pub const DEFAULT: WorkgroupSize = WorkgroupSize(256);

    /// `n` as a workgroup size, or `None` for zero.
    pub fn new(n: u32) -> Option<Self> {
        (n > 0).then_some(WorkgroupSize(n))
    }

    pub fn get(self) -> u32 {
        self.0
    }

    /// The workgroup size `v` names, for the primitive `who`: a `:type-error`
    /// for a value that is not an integer, and a `:value-error` for an
    /// integer that is not a positive 32-bit one. The pair is the error's kind
    /// and message.
    pub(crate) fn of_value(v: Value, who: &str) -> Result<Self, (&'static str, String)> {
        let Some(n) = v.as_int() else {
            return Err((
                "type-error",
                format!(
                    "{who}: expected an integer workgroup size, got {}",
                    v.type_name()
                ),
            ));
        };
        u32::try_from(n)
            .ok()
            .and_then(WorkgroupSize::new)
            .ok_or_else(|| {
                (
                    "value-error",
                    format!("{who}: a workgroup size is a positive 32-bit integer, got {n}"),
                )
            })
    }
}

/// A `spirv_cache` entry: the kernels compiled for one code object, one per
/// workgroup size, plus the pin that keeps the keyed bytecode alive, exactly
/// as a `JitCacheEntry` does.
pub struct SpirvEntry {
    _pin: CodePin,
    kernels: Vec<(WorkgroupSize, Vec<u8>)>,
}

impl SpirvEntry {
    /// An entry with no kernel yet, held by `pin`, which names the entry's key.
    pub fn new(pin: CodePin) -> Self {
        SpirvEntry {
            _pin: pin,
            kernels: Vec::new(),
        }
    }

    /// The kernel compiled at `size`, if any.
    pub fn kernel(&self, size: WorkgroupSize) -> Option<&[u8]> {
        self.kernels
            .iter()
            .find(|(s, _)| *s == size)
            .map(|(_, bytes)| bytes.as_slice())
    }

    /// Hold `bytes` as the kernel compiled at `size`, in place of any earlier
    /// one.
    fn insert(&mut self, size: WorkgroupSize, bytes: Vec<u8>) {
        match self.kernels.iter_mut().find(|(s, _)| *s == size) {
            Some((_, held)) => *held = bytes,
            None => self.kernels.push((size, bytes)),
        }
    }
}

impl VM {
    /// The SPIR-V compiled for the code object `t` at workgroup size `size`,
    /// if any.
    pub fn spirv_for(
        &self,
        t: &crate::value::ClosureTemplate,
        size: WorkgroupSize,
    ) -> Option<&[u8]> {
        self.spirv_cache
            .get(&t.bytecode().as_ptr())
            .and_then(|e| e.kernel(size))
    }

    /// Whether any SPIR-V is cached for the code object `t`, at any size: the
    /// question the call path asks before a GIT'd closure runs, which needs
    /// the `:gpu` capability (docs/impl/spirv.md).
    pub fn has_spirv(&self, t: &crate::value::ClosureTemplate) -> bool {
        self.spirv_cache.contains_key(&t.bytecode().as_ptr())
    }

    /// Cache `bytes` as the SPIR-V compiled for the code object `t` at
    /// workgroup size `size`. The single write path into `spirv_cache`: the
    /// first kernel for `t` makes the entry, which pins `t`'s code region and
    /// derives its key from the pin (docs/impl/jit.md).
    pub fn install_spirv(
        &mut self,
        t: &crate::value::ClosureTemplate,
        size: WorkgroupSize,
        bytes: Vec<u8>,
    ) {
        let key = t.bytecode().as_ptr();
        if !self.spirv_cache.contains_key(&key) {
            let pin = CodePin::of(self.heap(), t);
            self.spirv_cache.insert(pin.key(), SpirvEntry::new(pin));
        }
        self.spirv_cache
            .get_mut(&key)
            .expect("the entry for `t` exists, made above if it was missing")
            .insert(size, bytes);
    }

    /// Drop every cache entry that pins a code region: the JIT cache, the
    /// compiles in flight, the JIT rejections, the SPIR-V cache, the MLIR
    /// tier's engines and rejections, and the WASM tier's modules and
    /// rejections. Teardown runs this before it releases the process roots,
    /// so no pin holds a region past the sweep (docs/impl/jit.md).
    pub fn clear_code_pins(&mut self) {
        #[cfg(feature = "jit")]
        {
            self.jit_cache.clear();
            self.jit_pending.clear();
            self.jit_rejections.clear();
        }
        self.spirv_cache.clear();
        #[cfg(feature = "mlir")]
        if let Some(cache) = self.mlir_cache.as_mut() {
            cache.clear_pins();
        }
        #[cfg(feature = "wasm")]
        {
            if let Some(tier) = self.wasm_tier.as_mut() {
                tier.clear_pins();
            }
            self.wasm_rejections.clear();
        }
    }

    /// Record a closure call and return whether it is hot: called at least the
    /// JIT threshold's number of times (ten by default; `(vm/config-set :jit N)`
    /// sets it).
    pub fn record_closure_call(&mut self, bytecode_ptr: *const u8) -> bool {
        let count = self.closure_call_counts.entry(bytecode_ptr).or_insert(0);
        *count += 1;
        *count >= self.runtime_config.jit.threshold()
    }

    /// Get call count for a closure
    pub fn get_closure_call_count(&self, bytecode_ptr: *const u8) -> usize {
        self.closure_call_counts
            .get(&bytecode_ptr)
            .copied()
            .unwrap_or(0)
    }
}
