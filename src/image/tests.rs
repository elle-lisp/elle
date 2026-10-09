// audited: 2026-10-06
//! A closure's LIR in the body: the bytes the dumper writes for it, and the
//! extents the verifier refuses.
//!
//! docs/impl/image/sealing.md
//! docs/impl/image/format.md

use std::path::Path;

use crate::lir::code::{BlockRec, ConstRec, Node, SiteRec};
use crate::pipeline::eval_all;
use crate::runtime::Runtime;
use crate::symbol::SymbolTable;
use crate::value::fiberheap::FiberHeap;
use crate::value::region_slice::RegionSlice;
use crate::value::Value;

use super::{ImageError, Sections};

/// A compiled lambda with every kind of LIR record: blocks, immediate
/// constants, and a yield point. Its body uses intrinsics alone, so its graph
/// is its own and holds no stdlib closure.
const LAMBDA: &str = "(fn [x] (if (%eq x 2) (yield 42) 7))";

fn compiled(rt: &mut Runtime) -> Value {
    let (vm, symbols, cctx) = rt.parts();
    eval_all(LAMBDA, symbols, vm, cctx, "<image-lir>").expect("the lambda compiles")
}

fn dump(rt: &mut Runtime, root: Value, path: &Path) {
    let (heap, symbols) = rt.heap_and_symbols();
    super::dump(heap, symbols, root, path).expect("dump");
}

fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().expect("8 bytes"))
}

/// Two dumps of one closure write one file, whatever the pad bytes of its LIR
/// records hold. Freezing writes every pad as zero, so a pad that holds
/// anything else is residue the dumper must not carry into the artifact.
///
/// The counter-factual is a dumper that copies the records as raw bytes: the
/// first dump matches, and the second carries the poison below.
#[test]
fn two_dumps_write_one_file_whatever_the_lir_pads_hold() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut rt = Runtime::new();
    let f = compiled(&mut rt);
    let view = f
        .as_closure()
        .expect("a closure")
        .template
        .lir()
        .expect("a compiled lambda's payload carries LIR");
    let blocks = view.block_records();
    let consts = view.const_records();
    let [yields, calls] = view.site_records();
    assert!(
        !blocks.is_empty() && !consts.is_empty() && !yields.is_empty(),
        "the lambda must carry blocks, constants and a yield point, or the \
         poison below reaches nothing"
    );

    let a = dir.path().join("a.image");
    dump(&mut rt, f, &a);

    // SAFETY: the records sit in the payload's region pages, which this
    // test's heap owns and nothing reads concurrently. Only the pad bytes
    // change, and nothing reads a pad.
    unsafe {
        for rec in blocks {
            (*(rec as *const _ as *mut BlockRec)).pad = [0xA5; 3];
        }
        for rec in consts {
            (*(rec as *const _ as *mut ConstRec)).pad = [0xA5; 7];
        }
        for rec in yields.iter().chain(calls) {
            (*(rec as *const _ as *mut SiteRec)).pad = 0xA5A5;
        }
    }

    let b = dir.path().join("b.image");
    dump(&mut rt, f, &b);
    assert_eq!(
        std::fs::read(&a).expect("read a"),
        std::fs::read(&b).expect("read b"),
        "a pad byte in a live LIR record reached the image"
    );
}

/// The verifier bounds every slice of a payload's LIR body, and names the one
/// that leaves the image. A node table's length is the field damaged here:
/// a decoder trusts it to bound every node index, so an image whose length
/// overruns would have the JIT read past the image's pages.
#[test]
fn an_lir_slice_that_leaves_the_image_is_refused_by_name() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut rt = Runtime::new();
    let f = compiled(&mut rt);
    let nodes: Vec<Node> = f
        .as_closure()
        .expect("a closure")
        .template
        .lir()
        .expect("a compiled lambda's payload carries LIR")
        .nodes()
        .map(|n| *n.record())
        .collect();
    // SAFETY: a `Node` is plain `repr(C)` data with no padding.
    let node_bytes = unsafe {
        std::slice::from_raw_parts(
            nodes.as_ptr() as *const u8,
            nodes.len() * std::mem::size_of::<Node>(),
        )
    };

    let path = dir.path().join("lir.image");
    dump(&mut rt, f, &path);
    let mut bytes = std::fs::read(&path).expect("read the image");
    let s: Sections = super::sections(&bytes).expect("a fresh image parses");

    // The node table's slot is the relocation whose target holds exactly the
    // source function's nodes: a node holds no pointer, so its bytes cross
    // unchanged.
    let slot = s
        .relocations
        .clone()
        .step_by(Sections::RELOC_BYTES)
        .map(|e| (u64_at(&bytes, e) as usize, u64_at(&bytes, e + 8) as usize))
        .find(|&(_, target)| {
            let at = s.pages.start + target;
            bytes.get(at..at + node_bytes.len()) == Some(node_bytes)
        })
        .map(|(slot, _)| slot)
        .expect("a relocation names the node table");
    let (ptr_at, len_at, len_size) = RegionSlice::<u8>::header_layout();
    let len = s.pages.start + slot - ptr_at + len_at;
    bytes[len..len + len_size].fill(0xFF);
    std::fs::write(&path, &bytes).expect("write the damaged image");

    let mut heap = FiberHeap::new();
    let before = heap.active_region_count();
    match super::hydrate_path(&mut heap, &mut SymbolTable::new(), &path) {
        Err(ImageError::Corrupt(what)) => assert!(
            what.contains("lir") && what.contains("nodes"),
            "the refusal does not name the LIR's node table: {what}"
        ),
        Err(other) => panic!("expected a corrupt-image refusal, got {other:?}"),
        Ok(_) => panic!("an image whose LIR node table leaves it hydrated"),
    }
    assert_eq!(
        heap.active_region_count(),
        before,
        "the refused image left a region behind"
    );
}
