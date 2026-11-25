// audited: 2026-11-25
//! Bytecode operand readers: each decodes one big-endian operand or list and advances
//! the instruction pointer.

use super::*;

impl VM {
    #[inline(always)]
    pub fn read_u8(&self, bytecode: &[u8], ip: &mut usize) -> u8 {
        let val = bytecode[*ip];
        *ip += 1;
        val
    }

    #[inline(always)]
    pub fn read_u16(&self, bytecode: &[u8], ip: &mut usize) -> u16 {
        let high = bytecode[*ip] as u16;
        let low = bytecode[*ip + 1] as u16;
        *ip += 2;
        (high << 8) | low
    }

    #[inline(always)]
    pub fn read_i16(&self, bytecode: &[u8], ip: &mut usize) -> i16 {
        self.read_u16(bytecode, ip) as i16
    }

    /// Read a u32 (big-endian) region-id operand.
    #[inline(always)]
    pub fn read_u32(&self, bytecode: &[u8], ip: &mut usize) -> u32 {
        let b0 = bytecode[*ip] as u32;
        let b1 = bytecode[*ip + 1] as u32;
        let b2 = bytecode[*ip + 2] as u32;
        let b3 = bytecode[*ip + 3] as u32;
        *ip += 4;
        (b0 << 24) | (b1 << 16) | (b2 << 8) | b3
    }

    /// Read a `StaticRegion` operand — a region-bearing instruction's compile-time
    /// slot, decoded back into its newtype.
    ///
    /// The emitter writes every region operand as `StaticRegion::get()`, which is
    /// `NonZeroU32` by construction (there is no slot 0 — "no region" is encoded
    /// structurally by a variant *without* a region field, never as an in-band
    /// sentinel). So decoding can only ever see a nonzero slot; a 0 here means the
    /// emitter broke that invariant, and the `expect` turns that into a loud panic
    /// at the boundary rather than letting a phantom slot leak downstream. Keeping
    /// the slot a `StaticRegion` (not a bare `u32`) is what makes a static-vs-runtime
    /// region comparison a *compile* error — see `dispatch_native_call`.
    #[inline(always)]
    pub fn read_static_region(&self, bytecode: &[u8], ip: &mut usize) -> StaticRegion {
        let raw = self.read_u32(bytecode, ip);
        StaticRegion::new(raw)
            .expect("region operand is nonzero — emitter writes StaticRegion::get()")
    }

    /// Read a `SignalBits` operand: eight bytes, big-endian.
    ///
    /// The counterpart of [`crate::compiler::bytecode::Bytecode::emit_signal_bits`].
    /// The full width matters: a `(signal :keyword)` declaration
    /// allocates bits 32-63, so a mask read at any narrower width names no
    /// user signal at all (docs/impl/bytecode.md).
    #[inline(always)]
    pub fn read_signal_bits(&self, bytecode: &[u8], ip: &mut usize) -> SignalBits {
        let raw = u64::from_be_bytes(bytecode[*ip..*ip + 8].try_into().expect("8 bytes"));
        *ip += 8;
        SignalBits::new(raw)
    }

    /// Read a u64 (big-endian) operand — a `SymbolId` name hash.
    /// The counterpart of [`crate::compiler::bytecode::Bytecode::emit_u64`].
    #[inline(always)]
    pub fn read_u64(&self, bytecode: &[u8], ip: &mut usize) -> u64 {
        let raw = u64::from_be_bytes(bytecode[*ip..*ip + 8].try_into().expect("8 bytes"));
        *ip += 8;
        raw
    }

    #[inline(always)]
    pub fn read_i32(&self, bytecode: &[u8], ip: &mut usize) -> i32 {
        let b0 = bytecode[*ip] as u32;
        let b1 = bytecode[*ip + 1] as u32;
        let b2 = bytecode[*ip + 2] as u32;
        let b3 = bytecode[*ip + 3] as u32;
        *ip += 4;
        ((b0 << 24) | (b1 << 16) | (b2 << 8) | b3) as i32
    }
}

/// A list an instruction carries in its bytecode: a count byte, then `N` bytes
/// per entry. The VM reads it where it lies, so a call copies none of it
/// (docs/impl/spares.md).
#[derive(Clone, Copy)]
pub(crate) struct CodeList<'a, const N: usize>(&'a [u8]);

impl<'a, const N: usize> CodeList<'a, N> {
    /// A list with no entries.
    pub(crate) const NONE: CodeList<'static, N> = CodeList(&[]);

    /// Read the list at `ip`, and step `ip` past it.
    pub(crate) fn read(bytecode: &'a [u8], ip: &mut usize) -> Self {
        let start = *ip + 1;
        *ip = start + N * bytecode[*ip] as usize;
        CodeList(&bytecode[start..*ip])
    }

    /// Each entry's bytes, in order.
    pub(crate) fn entries(self) -> impl Iterator<Item = [u8; N]> + 'a {
        self.0.as_chunks::<N>().0.iter().copied()
    }

    pub(crate) fn is_empty(self) -> bool {
        self.0.is_empty()
    }
}

impl<'a> CodeList<'a, 4> {
    /// Each entry as the static region slot it names.
    pub(crate) fn slots(self) -> impl Iterator<Item = StaticRegion> + 'a {
        self.entries()
            .map(|word| StaticRegion::new(u32::from_be_bytes(word)).expect("a slot is nonzero"))
    }
}
