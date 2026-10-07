// audited: 2026-10-06
//! The records carry no padding the compiler chose, and a long operand list lands in the pool.
//!
//! docs/impl/lir.md

use super::*;
use std::mem::{size_of, size_of_val};

/// The sum of the named fields' sizes. A field added to a record and left out
/// here makes the record larger than the sum, so the list cannot fall behind.
macro_rules! fields {
    ($rec:expr; $($f:ident),* $(,)?) => {
        0 $(+ size_of_val(&$rec.$f))*
    };
}

/// A record whose size exceeds its fields' carries implicit padding: bytes no
/// field names, which no writer sets, and which a dump would copy into the
/// image as whatever the allocator left there. The counter-factual is a record
/// the compiler pads, which writes two different files for one function.
#[test]
fn every_record_is_exactly_its_fields() {
    let n = Node::default();
    assert_eq!(
        size_of::<Node>(),
        fields!(n; start, end, line, col, file, dst, region, aux, uses, extra, n_uses, op, flags)
    );
    assert_eq!(size_of::<Node>(), 48, "a node is the prototype's 48 bytes");

    let b = BlockRec::default();
    assert_eq!(
        size_of::<BlockRec>(),
        fields!(b; label, first, len, term_a, term_b, term_c, start, end, line, col, file, term_op, pad)
    );

    let c = ConstRec::default();
    assert_eq!(size_of::<ConstRec>(), fields!(c; kind, pad, bits));

    let s = SiteRec::default();
    assert_eq!(
        size_of::<SiteRec>(),
        fields!(s; resume_ip, num_locals, pad, regs, n_regs)
    );
}

/// The first two uses ride in the node and the rest in the pool, so a node is
/// one size however many operands its instruction reads. A use list longer
/// than two is whole in the pool, so a reader slices it in one piece.
#[test]
fn the_third_and_later_uses_land_in_the_pool() {
    let elements = vec![r(2), r(3), r(4), r(5)];
    let owned = frozen(&[
        InstrRef::MakeArrayMut {
            dst: r(1),
            elements: &elements,
            region: slot(21),
        },
        InstrRef::Put {
            dst: r(6),
            obj: r(7),
            key: r(8),
            val: r(9),
        },
        InstrRef::List {
            dst: r(10),
            head: r(11),
            tail: r(12),
            region: slot(22),
        },
    ]);
    let view = owned.view();
    let pool = view.pool();
    let nodes: Vec<NodeRef<'_>> = view.nodes().collect();

    let array = nodes[0].record();
    assert_eq!(array.n_uses, 4);
    let at = array.extra as usize;
    let words: Vec<u32> = elements.iter().map(|r| r.0).collect();
    assert_eq!(
        &pool[at..at + 4],
        &words[..],
        "all four elements in the pool"
    );
    assert_eq!(nodes[0].uses(), &elements[..]);

    let put = nodes[1].record();
    assert_eq!(put.n_uses, 3);
    let at = put.extra as usize;
    assert_eq!(
        &pool[at..at + 3],
        &[7, 8, 9],
        "a third fixed use is pooled too"
    );

    let pair = nodes[2].record();
    assert_eq!(pair.n_uses, 2);
    assert_eq!(pair.uses, [r(11), r(12)], "two uses stay in the node");
}

/// Freezing writes every byte it owns, so one function freezes to one set of
/// bytes. The counter-factual is a record built field by field over memory
/// nobody cleared, which differs between two freezes of one function.
#[test]
fn one_function_freezes_to_one_set_of_bytes() {
    let instrs: Vec<InstrRef<'_>> = Op::ALL.iter().map(|&op| exemplar(op)).collect();
    let a = frozen(&instrs);
    let b = frozen(&instrs);
    assert!(!a.code().nodes.is_empty());
    assert_eq!(bytes(&a.code().nodes), bytes(&b.code().nodes));
    assert_eq!(bytes(&a.code().blocks), bytes(&b.code().blocks));
    assert_eq!(bytes(&a.code().consts), bytes(&b.code().consts));
    assert_eq!(a.code().pool, b.code().pool);
    assert_eq!(a.code().data, b.code().data);
}

/// A record slice's bytes. Sound only for the records above, which
/// `every_record_is_exactly_its_fields` shows hold no padding to read.
fn bytes<T: Copy>(records: &[T]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(records.as_ptr() as *const u8, size_of_val(records)) }
}
