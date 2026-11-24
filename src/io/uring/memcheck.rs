// audited: 2026-11-23
//! Memcheck's client request, which marks the bytes the kernel wrote through the ring as defined.
//!
//! docs/impl/io/bytes.md

/// Memcheck's request that marks memory defined: its tool base, made of `M`
/// and `C`, and the request's index in `memcheck.h`.
const MAKE_MEM_DEFINED: u64 = ((b'M' as u64) << 24 | (b'C' as u64) << 16) + 2;

/// Tell memcheck that the kernel wrote `bytes` through the ring, which it
/// cannot see. Outside valgrind the request leaves every register as it found
/// it, and does nothing.
pub(super) fn kernel_wrote(bytes: &[u8]) {
    let args: [u64; 6] = [
        MAKE_MEM_DEFINED,
        bytes.as_ptr() as u64,
        bytes.len() as u64,
        0,
        0,
        0,
    ];
    // SAFETY: the four rotations turn rdi a whole 128 bits, and the exchange
    // of rbx with itself changes nothing. Valgrind reads the request through
    // rax and answers in rdx.
    #[cfg(target_arch = "x86_64")]
    unsafe {
        std::arch::asm!(
            "rol rdi, 3",
            "rol rdi, 13",
            "rol rdi, 61",
            "rol rdi, 51",
            "xchg rbx, rbx",
            in("rax") args.as_ptr(),
            inout("rdx") 0u64 => _,
        );
    }
    // SAFETY: the four rotations turn x12 a whole 128 bits, and the `orr` of
    // x10 with itself changes nothing. Valgrind reads the request through x4
    // and answers in x3.
    #[cfg(target_arch = "aarch64")]
    unsafe {
        std::arch::asm!(
            "ror x12, x12, #3",
            "ror x12, x12, #13",
            "ror x12, x12, #51",
            "ror x12, x12, #61",
            "orr x10, x10, x10",
            in("x4") args.as_ptr(),
            inout("x3") 0u64 => _,
        );
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    let _ = args;
}
