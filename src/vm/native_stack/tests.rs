// audited: 2026-11-23
//! The native-stack probe tracks the frames a thread holds, and the check
//! against a VM's stack floor is one compare.

use super::*;

#[test]
fn this_platform_reports_how_much_stack_is_left() {
    let left = remaining().expect("Linux, macOS and Android report the thread's stack bounds");
    assert!(left > 0, "a running thread has some stack left");
}

/// Nest `levels` frames of 4 KiB each and read the probe at the bottom.
#[inline(never)]
fn remaining_under(levels: usize) -> usize {
    let pad = std::hint::black_box([0u8; 4096]);
    if levels == 0 {
        remaining().expect("the probe answers")
    } else {
        // Adding `pad[0]` after the call keeps the recursion out of tail
        // position, so every level holds its frame while the next one runs.
        remaining_under(levels - 1) + pad[0] as usize
    }
}

// Counter-factual: a probe that read a fixed number, or the stack's size
// rather than its position, would report the same answer at both depths.
#[test]
fn what_is_left_shrinks_as_frames_nest() {
    let outer = remaining().expect("the probe answers");
    let inner = remaining_under(16);
    assert!(
        outer >= inner + 16 * 4096,
        "16 nested 4 KiB frames should cost at least 64 KiB: {outer} bytes left \
         outside, {inner} inside"
    );
}

// The trap: a thread may get MORE stack than it asked for. glibc keeps the
// stacks of exited threads and hands a new thread any cached one at least as
// large as the request, so a 4 MiB request can come back 8 MiB and the probe
// rightly reports that. Only the lower bound is a claim about the probe, and a
// request far above the main thread's 8 MiB is one no cached stack satisfies —
// a probe that read the main thread's bounds instead would fail it.
#[test]
fn a_spawned_thread_reports_the_stack_it_was_given() {
    const SIZE: usize = 100 * 1024 * 1024;
    let left = std::thread::Builder::new()
        .stack_size(SIZE)
        .spawn(remaining)
        .expect("spawn")
        .join()
        .expect("join")
        .expect("the probe answers on a spawned thread");
    assert!(
        left > SIZE - 512 * 1024,
        "a thread given {SIZE} bytes of stack reports only {left} bytes left at its start"
    );
}

/// A floor no thread's stack sits at, and the compiled-call reserve, for the
/// compare alone.
const FLOOR: usize = 0x7000_0000_0000;
const RESERVE: usize = 512 * 1024;

// Counter-factual: a compare that never reads low passes every other test in
// this file, and lets a compiled recursion overflow the thread.
#[test]
fn a_position_inside_the_reserve_reads_low() {
    assert!(
        reads_low(FLOOR, FLOOR, RESERVE),
        "at the floor, nothing is left"
    );
    assert!(reads_low(FLOOR + 1024, FLOOR, RESERVE), "1 KiB is left");
    assert!(
        !reads_low(FLOOR + RESERVE, FLOOR, RESERVE),
        "exactly the reserve is left, which is not less than the reserve"
    );
}

// Counter-factual: a saturating subtraction reads a position below the floor
// as nothing left, and so as low. That is a VM run on a coroutine's stack, which
// would then run every compiled callee interpreted and halt every re-entry.
#[test]
fn a_position_below_the_floor_never_reads_low() {
    assert!(!reads_low(FLOOR - 1, FLOOR, RESERVE));
    assert!(!reads_low(FLOOR - 4 * RESERVE, FLOOR, RESERVE));
}

// Counter-factual: a check that read a position outside the thread's stack as
// low, to fall back to the interpreter when unsure, fails here.
#[test]
fn a_position_above_the_stack_never_reads_low() {
    assert!(!reads_low(FLOOR + (1 << 30), FLOOR, RESERVE));
}

// Counter-factual: a check that read an unknown floor as no room fails here,
// and would run every call interpreted where the platform reports no bounds.
#[test]
fn a_floor_of_zero_never_reads_low() {
    assert!(!StackFloor(0).below(RESERVE));
}

// Counter-factual: a floor shared across threads, read by whichever VM was
// built first. The VM built on this thread first makes that floor this thread's,
// which lies megabytes from any position on the spawned thread's stack. The
// probe is the oracle: its own tests above tie it to the running thread.
#[test]
fn a_vm_records_the_floor_of_the_thread_that_built_it() {
    const SIZE: usize = 100 * 1024 * 1024;
    // The probe and the check read positions a few frames apart.
    const SLACK: usize = 64 * 1024;
    let _first = crate::vm::VM::new();
    let (left, short, ample) = std::thread::Builder::new()
        .stack_size(SIZE)
        .spawn(|| {
            let vm = crate::vm::VM::new();
            let left = remaining().expect("the probe answers on a spawned thread");
            (
                left,
                vm.stack_floor.below(left - SLACK),
                vm.stack_floor.below(left + SLACK),
            )
        })
        .expect("spawn")
        .join()
        .expect("join");
    assert!(
        !short,
        "{left} bytes are left, yet the check reads less than {} left",
        left - SLACK
    );
    assert!(
        ample,
        "{left} bytes are left, yet the check reads {} or more left",
        left + SLACK
    );
}

// Counter-factual: an offset that forgot the floor's place inside the `VM`
// reads another field, and the direct self-call's stack guard compares the
// stack pointer with that field.
#[cfg(feature = "jit")]
#[test]
fn the_floor_sits_at_the_offset_compiled_code_reads() {
    let vm = crate::vm::VM::new();
    let base = &vm as *const crate::vm::VM as *const u8;
    // SAFETY: the offset lies inside the `VM`, and names a `usize`.
    let read = unsafe { *(base.add(StackFloor::WORD_OFFSET_IN_VM) as *const usize) };
    assert_eq!(
        read, vm.stack_floor.0,
        "the word at the offset is the floor"
    );
    assert_ne!(
        read, 0,
        "this platform reports a floor, so the test reads one"
    );
}
