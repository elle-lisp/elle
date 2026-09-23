//! audited: 2026-09-23
//! A stream read or write on the thread pool stages none of its bytes on the
//! Rust heap, the pool worker's own included.
//!
//! docs/impl/io-bytes.md
//
// The thread-pool twin of `io_copies.rs`, in a binary of its own because the
// backend is a process-wide choice made once at startup. The counter counts
// every thread, so a worker that reads into a `Vec` of its own before the
// completion copies it is caught here as surely as a copy on the scheduler.
// Staged through a `Vec`, the pool measured 3.0 bytes per byte for a read and
// for a read after a read-line, 4.0 for a read followed by a write, and 2.9 for
// a read-all.

#[path = "common/mod.rs"]
mod common;
#[path = "io_copies/measure.rs"]
mod measure;

#[global_allocator]
static ALLOCATOR: measure::Counting = measure::Counting;

const BACKEND: &str = "the thread pool";

#[test]
fn a_pool_read_allocates_nothing_per_byte_it_answers_with() {
    measure::configure(true);
    measure::assert_slope_below(&measure::READ, 0.25, BACKEND);
}

#[test]
fn a_pool_read_after_an_overshoot_allocates_nothing_per_byte() {
    measure::configure(true);
    measure::assert_slope_below(&measure::READ_AFTER_LINE, 0.25, BACKEND);
}

#[test]
fn a_pool_write_allocates_nothing_per_byte_it_sends() {
    measure::configure(true);
    measure::assert_slope_below(&measure::WRITE, 0.25, BACKEND);
}

#[test]
fn a_pool_read_all_stages_its_bytes_once() {
    measure::configure(true);
    measure::assert_slope_below(&measure::READ_ALL, 1.25, BACKEND);
}
