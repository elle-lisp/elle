//! audited: 2026-09-23
//! A stream read or write on the platform's own backend stages none of its
//! bytes on the Rust heap.
//!
//! docs/impl/io-bytes.md
//
// Its own binary: the gauge is the global allocator, one counter for the whole
// process (docs/analysis/testing.md § "Process-global state needs its own
// binary"). On Linux this is the ring; `io_copies_pool.rs` is the same
// measurement on the thread pool, which a process chooses once, at startup.
//
// The counter-factual is the copy each limit forbids. Staged through a `Vec`,
// the ring measured 2.9 bytes per byte for a read, 3.0 for a read after a
// read-line, 4.9 for a read followed by a write, and 4.1 for a read-all. A
// read-all that stages once, in a buffer sized to the file, costs one.

#[path = "common/mod.rs"]
mod common;
#[path = "io_copies/measure.rs"]
mod measure;

#[global_allocator]
static ALLOCATOR: measure::Counting = measure::Counting;

const BACKEND: &str = "the platform default";

#[test]
fn a_read_allocates_nothing_per_byte_it_answers_with() {
    measure::configure(false);
    measure::assert_slope_below(&measure::READ, 0.25, BACKEND);
}

#[test]
fn a_read_after_an_overshoot_allocates_nothing_per_byte() {
    measure::configure(false);
    measure::assert_slope_below(&measure::READ_AFTER_LINE, 0.25, BACKEND);
}

#[test]
fn a_write_allocates_nothing_per_byte_it_sends() {
    measure::configure(false);
    measure::assert_slope_below(&measure::WRITE, 0.25, BACKEND);
}

#[test]
fn a_read_all_stages_its_bytes_once() {
    measure::configure(false);
    measure::assert_slope_below(&measure::READ_ALL, 1.25, BACKEND);
}
