// audited: 2026-09-23
//! The native-stack probe answers on this platform, and its answer tracks the
//! frames a thread actually holds.

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

#[test]
fn below_compares_what_is_left_with_the_reserve() {
    assert!(!below(0), "no thread has less than zero bytes left");
    assert!(
        below(usize::MAX),
        "every thread has less than usize::MAX left"
    );
}
