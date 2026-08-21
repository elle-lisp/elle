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
