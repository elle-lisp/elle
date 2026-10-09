// audited: 2026-10-06
//! A `RegionVec` answers as a `Vec` does, and a builder builds in its heap's pages and leaves no region behind.
//!
//! docs/impl/lir.md

use super::*;
use crate::lir::code::{ConstRef, Node};
use crate::lir::Reg;

/// A small deterministic generator, so a failing sequence replays.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Run `steps` random pushes, truncations and insertions against a
/// `RegionVec` and a `Vec` side by side, checking after every step that the
/// two hold the same items. `make` turns a counter into a distinct item.
fn against_the_model<T: Copy + PartialEq + std::fmt::Debug + 'static>(
    seed: u64,
    steps: usize,
    make: impl Fn(u64) -> T,
) {
    let mut heap = FiberHeap::new();
    let arena = LirArena::mint(&mut heap);
    let mut rng = Rng(seed);
    let mut model: Vec<T> = Vec::new();
    let mut grown: RegionVec<T> = RegionVec::new(arena);
    let mut counter = 0u64;
    for step in 0..steps {
        match rng.below(10) {
            0..=5 => {
                counter += 1;
                model.push(make(counter));
                grown.push(make(counter));
            }
            6 => {
                let len = if model.is_empty() {
                    0
                } else {
                    rng.below(model.len() + 1)
                };
                model.truncate(len);
                grown.truncate(len);
            }
            _ => {
                let at = rng.below(model.len() + 1);
                let run: Vec<T> = (0..rng.below(9))
                    .map(|_| {
                        counter += 1;
                        make(counter)
                    })
                    .collect();
                model.splice(at..at, run.iter().copied());
                grown.insert_slice(at, &run);
            }
        }
        assert_eq!(
            grown.as_slice(),
            &model[..],
            "seed {seed}: the slice and the model part at step {step}"
        );
    }
    heap.decref_region_if_present(arena.region());
}

/// The counter-factual is a slice that loses its tail when it grows into a
/// fresh extent, or an insertion that moves the tail by the wrong distance:
/// both answer correctly until the first growth or the first insertion, so the
/// sequence has to cross many of each.
#[test]
fn a_region_vec_answers_as_a_vec_does() {
    for seed in [1, 7, 0x5eed, 0xdead_beef] {
        against_the_model(seed, 3_000, |n| n as u32);
    }
}

/// A node is 48 bytes at four-byte alignment, and a value is sixteen at
/// eight: the extent a slice grows into must hold either type aligned.
#[test]
fn a_region_vec_holds_nodes_and_wider_items() {
    against_the_model(11, 2_000, |n| Node {
        dst: n as u32,
        aux: !(n as u32),
        ..Node::default()
    });
    against_the_model(12, 2_000, |n| (n, !n));
}

/// Growth past a page claims more pages, and the slice still answers every
/// item it was given.
#[test]
fn a_region_vec_grows_across_pages() {
    let mut heap = FiberHeap::new();
    let arena = LirArena::mint(&mut heap);
    let claims = heap.page_claims();
    let mut grown: RegionVec<u64> = RegionVec::new(arena);
    let n = 200_000u64;
    for i in 0..n {
        grown.push(i);
    }
    assert_eq!(grown.len(), n as usize);
    assert!(
        grown.as_slice().iter().copied().eq(0..n),
        "every pushed item reads back in order"
    );
    assert!(
        heap.page_claims() > claims + 1,
        "1.6 MB of items claim more than one page"
    );
    heap.decref_region_if_present(arena.region());
}

/// Every extent a slice claims lies in the region its arena names, so freeing
/// that region frees the slice. The counter-factual is a slice that falls back
/// to the Rust heap when it grows, which no region gauge would see.
#[test]
fn a_region_vec_lives_in_its_own_region() {
    let mut heap = FiberHeap::new();
    let arena = LirArena::mint(&mut heap);
    let mut grown: RegionVec<u32> = RegionVec::new(arena);
    for i in 0..10_000u32 {
        grown.push(i);
        if i.is_power_of_two() {
            // An empty slice's pointer is dangling, and `region_of_ptr` reads
            // the page header under it, so ask only about a slice that holds
            // what was pushed.
            assert_eq!(grown.len(), i as usize + 1, "the slice holds every push");
            let ptr = grown.as_slice().as_ptr() as *const ();
            assert_eq!(
                heap.region_of_ptr(ptr),
                arena.region().get(),
                "after {i} pushes the slice lies in its own region"
            );
        }
    }
    heap.decref_region_if_present(arena.region());
}

/// One function of `n` constant loads, built and frozen.
fn build_constants(builder: &mut LirBuilder<'_>, n: u32) -> LirOwned {
    builder.begin_function(Arity::Exact(0));
    builder.open_block(Label(0));
    for i in 0..n {
        builder.emit(
            InstrRef::Const {
                dst: Reg(i),
                value: ConstRef::Int(i as i64),
            },
            Span::synthetic(),
        );
    }
    builder.terminate(Terminator::Return(Reg(0)), Span::synthetic());
    builder.finish_block();
    builder.head().num_regs = n;
    builder
        .finish_function()
        .expect("a function of constants freezes")
}

/// A builder claims its pages from the heap it was given, and dropping it
/// leaves that heap holding the regions it held before. The counter-factual is
/// a builder that keeps its working form on the Rust heap: it claims no page,
/// and no gauge the region system owns can see what it built.
#[test]
fn a_builder_builds_in_its_heaps_pages_and_frees_them() {
    let mut heap = FiberHeap::new();
    let regions = heap.active_region_count();
    let claims = heap.page_claims();
    let frozen = {
        let mut builder = LirBuilder::new(&mut heap);
        build_constants(&mut builder, 4_000)
    };
    assert_eq!(frozen.view().nodes().count(), 4_000);
    assert!(
        heap.page_claims() > claims,
        "4,000 nodes claim pages from the builder's heap"
    );
    assert_eq!(
        heap.active_region_count(),
        regions,
        "the dropped builder leaves no working region behind"
    );
}

/// A function begun while another's block is half built leaves that block as
/// it was: the lowerer lowers a lambda's body at the `MakeClosure` that builds
/// it, in the middle of its parent's block.
#[test]
fn a_nested_function_leaves_its_parents_open_block_alone() {
    let mut heap = FiberHeap::new();
    let mut builder = LirBuilder::new(&mut heap);
    builder.begin_function(Arity::Exact(0));
    builder.open_block(Label(0));
    builder.emit(InstrRef::LoadSelf { dst: Reg(0) }, Span::synthetic());
    let inner = build_constants(&mut builder, 3);
    builder.emit(InstrRef::LoadSelf { dst: Reg(1) }, Span::synthetic());
    builder.terminate(Terminator::Return(Reg(1)), Span::synthetic());
    builder.finish_block();
    builder.head().num_regs = 2;
    let outer = builder.finish_function().expect("freezes");
    assert_eq!(inner.view().nodes().count(), 3);
    let ops: Vec<InstrRef<'_>> = outer.view().nodes().map(|n| n.instr()).collect();
    assert_eq!(
        ops,
        vec![
            InstrRef::LoadSelf { dst: Reg(0) },
            InstrRef::LoadSelf { dst: Reg(1) }
        ]
    );
}

/// A block finished with no `terminate` exits `Unreachable`.
#[test]
fn an_unterminated_block_is_unreachable() {
    let mut heap = FiberHeap::new();
    let mut builder = LirBuilder::new(&mut heap);
    builder.begin_function(Arity::Exact(0));
    builder.open_block(Label(3));
    builder.finish_block();
    let f = builder.finish_function().expect("freezes");
    assert_eq!(f.view().block(0).terminator(), Terminator::Unreachable);
    assert_eq!(f.view().block(0).label(), Label(3));
}
