// audited: 2026-11-11
//! The iovecs a gathered write hands the kernel: where its tail starts, and how much one write carries.
//!
//! docs/impl/io-bytes.md

use super::*;

/// Each iovec as the part it names and the bytes it carries: the part's index,
/// the offset into it, and the length. A part is found by its address, so an
/// iovec that names the right bytes from the wrong part fails here.
fn named(parts: &[&[u8]], done: usize, most: usize) -> Vec<(usize, usize, usize)> {
    tail_iovecs(parts, done, most)
        .map(|iov| {
            let at = iov.iov_base as usize;
            let (index, part) = parts
                .iter()
                .enumerate()
                .find(|(_, p)| {
                    let start = p.as_ptr() as usize;
                    !p.is_empty() && at >= start && at < start + p.len()
                })
                .expect("an iovec names bytes no part holds");
            (index, at - part.as_ptr() as usize, iov.iov_len)
        })
        .collect()
}

#[test]
fn the_tail_starts_inside_the_part_the_sent_bytes_reach() {
    let (a, b, c) = (vec![b'a'; 10], vec![b'b'; 5], vec![b'c'; 7]);
    let parts: [&[u8]; 3] = [&a, &b, &c];
    assert_eq!(
        named(&parts, 0, usize::MAX),
        [(0, 0, 10), (1, 0, 5), (2, 0, 7)],
        "nothing sent: every part, whole",
    );
    assert_eq!(
        named(&parts, 10, usize::MAX),
        [(1, 0, 5), (2, 0, 7)],
        "the first part sent: the rest, whole",
    );
    assert_eq!(
        named(&parts, 12, usize::MAX),
        [(1, 2, 3), (2, 0, 7)],
        "two bytes into the second part: its last three, then the third",
    );
    assert_eq!(
        named(&parts, 21, usize::MAX),
        [(2, 6, 1)],
        "one byte left: the last byte of the last part",
    );
    assert!(
        named(&parts, 22, usize::MAX).is_empty(),
        "every byte sent: no iovec",
    );
}

#[test]
fn an_empty_part_names_no_iovec() {
    let (a, b) = (vec![b'a'; 3], vec![b'b'; 2]);
    let empty: &[u8] = &[];
    let parts: [&[u8]; 5] = [empty, &a, empty, &b, empty];
    assert_eq!(
        named(&parts, 0, usize::MAX),
        [(1, 0, 3), (3, 0, 2)],
        "an iovec of length zero would cost a slot of IOV_MAX for nothing",
    );
    assert_eq!(
        named(&parts, 3, usize::MAX),
        [(3, 0, 2)],
        "the tail after the first part skips the empty part between",
    );
}

#[test]
fn a_tail_carries_at_most_iov_max_iovecs() {
    let bytes: Vec<u8> = (0..3000).map(|i| i as u8).collect();
    let parts: Vec<&[u8]> = bytes.chunks(1).collect();
    let first = named(&parts, 0, usize::MAX);
    assert_eq!(first.len(), IOV_MAX, "the kernel refuses more than IOV_MAX");
    assert_eq!(first[0], (0, 0, 1));
    assert_eq!(first[IOV_MAX - 1], (IOV_MAX - 1, 0, 1));
    let later = named(&parts, 1500, usize::MAX);
    assert_eq!(later.len(), IOV_MAX);
    assert_eq!(
        later[0],
        (1500, 0, 1),
        "a later write starts where the sent bytes end"
    );
    assert_eq!(
        named(&parts, 2500, usize::MAX).len(),
        500,
        "the last write carries what is left",
    );
}

#[test]
fn a_tail_carries_at_most_the_bytes_one_write_takes() {
    let (a, b, c) = (vec![b'a'; 10], vec![b'b'; 10], vec![b'c'; 10]);
    let parts: [&[u8]; 3] = [&a, &b, &c];
    assert_eq!(
        named(&parts, 0, 15),
        [(0, 0, 10), (1, 0, 5)],
        "the iovec that crosses the limit is cut at it",
    );
    assert_eq!(
        named(&parts, 5, 15),
        [(0, 5, 5), (1, 0, 10)],
        "the limit counts from the first byte not yet sent",
    );
    assert_eq!(
        named(&parts, 10, 10),
        [(1, 0, 10)],
        "a limit that ends on a part's last byte takes no iovec past it",
    );
}
