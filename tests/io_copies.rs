// audited: 2026-11-12
//! A stream read or write on the platform's own backend stages its bytes on the Rust heap only where it must, and then once.
//!
//! docs/impl/io/bytes.md
//
// Its own binary: the gauge is the global allocator, one counter for the whole
// process (docs/analysis/testing.md). The platform's own backend is the ring on
// Linux and the thread pool on a Mac or in a build without the `uring` feature,
// and the counter counts every thread, so a pool worker that reads into a `Vec`
// of its own is caught as surely as a copy on the scheduler.
//
// The counter-factual is the copy each limit forbids. Staged through a `Vec`,
// the ring measured 2.9 bytes per byte for a read, 3.0 for a read after a
// read-line, 4.9 for a read followed by a write, and 4.1 for a read-all; the
// pool measured 3.0, 3.0, 4.0 and 2.9. A read-all that stages once, in a
// buffer sized to the file, costs one. So does a gathered write of mutable
// parts, which copies them once; joining them first and then copying the join
// into a pooled buffer costs two.

#[path = "common/mod.rs"]
mod common;
#[path = "io_copies/measure.rs"]
mod measure;

#[global_allocator]
static ALLOCATOR: measure::Counting = measure::Counting;

const BACKEND: &str = "the platform's own backend";

#[test]
fn a_read_allocates_nothing_per_byte_it_answers_with() {
    measure::configure();
    measure::assert_slope_below(&measure::READ, 0.25, BACKEND);
}

#[test]
fn a_read_after_an_overshoot_allocates_nothing_per_byte() {
    measure::configure();
    measure::assert_slope_below(&measure::READ_AFTER_LINE, 0.25, BACKEND);
}

#[test]
fn a_write_allocates_nothing_per_byte_it_sends() {
    measure::configure();
    measure::assert_slope_below(&measure::WRITE, 0.25, BACKEND);
}

#[test]
fn a_gathered_write_allocates_nothing_per_byte_it_sends() {
    measure::configure();
    measure::assert_slope_below(&measure::WRITE_MANY, 0.25, BACKEND);
}

#[test]
fn a_gathered_write_of_mutable_parts_copies_them_once() {
    measure::configure();
    measure::assert_slope_below(&measure::WRITE_MANY_MUTABLE, 1.25, BACKEND);
}

#[test]
fn a_read_all_stages_its_bytes_once() {
    measure::configure();
    measure::assert_slope_below(&measure::READ_ALL, 1.25, BACKEND);
}
