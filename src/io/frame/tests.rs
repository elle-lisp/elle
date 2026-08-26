use super::*;
use crate::segment::Generation;

fn gen() -> Generation {
    crate::config::get().unicode_generation()
}

/// The newline is the boundary, and it is not part of the line.
#[test]
fn a_line_ends_at_its_newline_and_the_rest_stays_with_the_port() {
    assert_eq!(line_end(b"one\ntwo\n"), (3, 4));
    assert_eq!(line_end(b"\n"), (0, 1));
}

/// A `\r` before the newline belongs to the terminator, not to the line.
#[test]
fn a_carriage_return_before_the_newline_goes_with_it() {
    assert_eq!(line_end(b"one\r\ntwo"), (3, 5));
    // A lone `\r` inside the line is data.
    assert_eq!(line_end(b"o\rne\ntwo"), (4, 5));
}

/// A stream that ended mid-line answers with the whole partial line.
///
/// The trap: trimming a terminator that is not there would eat a byte of data.
/// Nothing follows, so the port keeps no remainder.
#[test]
fn a_partial_last_line_is_the_whole_answer() {
    assert_eq!(line_end(b"one"), (3, 3));
    assert_eq!(line_end(b""), (0, 0));
    assert_eq!(line_end(b"one\r"), (4, 4));
}

/// A binary port counts bytes.
#[test]
fn an_exact_read_of_bytes_ends_at_the_count() {
    assert_eq!(exact_end(b"abcdef", 4, Encoding::Binary, gen()), Some(4));
    assert_eq!(exact_end(b"abc", 4, Encoding::Binary, gen()), None);
    assert_eq!(exact_end(b"abcd", 4, Encoding::Binary, gen()), Some(4));
}

/// A text port counts grapheme clusters, and a cluster is as many bytes as it
/// takes.
///
/// The trap this pins: four bytes per cluster was once treated as a bound. One
/// family emoji is 25 bytes and one cluster, so two of them end 50 bytes in —
/// six times what the bound would have allowed for.
#[test]
fn an_exact_read_of_clusters_ends_wherever_the_clusters_do() {
    let family = "👨‍👩‍👧‍👦".as_bytes();
    assert_eq!(family.len(), 25);
    let two: Vec<u8> = [family, family].concat();
    assert_eq!(exact_end(&two, 1, Encoding::Text, gen()), Some(25));
    assert_eq!(exact_end(&two, 2, Encoding::Text, gen()), Some(50));
    assert_eq!(exact_end(&two, 3, Encoding::Text, gen()), None);
}
