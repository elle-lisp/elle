// audited: 2026-11-27
//! What a side exit stores: each local live at its site, and no dead one.
//!
//! docs/impl/jit-closure.md

use super::*;
use crate::hir::region::StaticRegion;
use crate::lir::Label;

/// fn() of `live` locals the first block stores and the last block reads,
/// then `dead` locals nothing names: `blocks` blocks in a chain, each calling
/// the function its capture holds. The function may yield, so each call
/// charges and parks, and every live local is live at each of those sites.
fn chain(blocks: u32, live: u16, dead: u16) -> LirOwned {
    let stores: Vec<InstrRef<'static>> = (0..live)
        .map(|slot| InstrRef::StoreLocal { slot, src: Reg(2) })
        .collect();
    let loads: Vec<InstrRef<'static>> = (0..live)
        .map(|slot| InstrRef::LoadLocal {
            dst: Reg(3 + slot as u32),
            slot,
        })
        .collect();
    let mut fixture = LirFixture::new(Arity::Exact(0))
        .signal(Signal::yields())
        .num_captures(1)
        .num_locals(live + dead);
    for block in 0..blocks {
        let mut instrs = Vec::new();
        // A constant no store consumes stays on the operand stack, and each
        // site would spill it as an operand in place of the first local.
        if block == 0 && live > 0 {
            instrs.push(InstrRef::Const {
                dst: Reg(2),
                value: crate::lir::ConstRef::Int(7),
            });
            instrs.extend(stores.iter().copied());
        }
        instrs.push(InstrRef::LoadCapture {
            dst: Reg(0),
            index: 0,
        });
        instrs.push(InstrRef::call(
            Reg(1),
            Reg(0),
            &[],
            false,
            StaticRegion::new(1).unwrap(),
        ));
        let terminator = if block + 1 == blocks {
            instrs.extend(loads.iter().copied());
            Terminator::Return(Reg(1))
        } else {
            Terminator::Jump(Label(block + 1))
        };
        fixture = fixture.block(block, &instrs, terminator);
    }
    fixture.emitted_sites().build()
}

/// How many `store` instructions the translation of `lir` holds.
fn stores(lir: &LirOwned) -> usize {
    verified()
        .clif_text(&lir.view(), crate::jit::ChildTable::EMPTY)
        .expect("Failed to translate")
        .iter()
        .filter(|line| line.trim().starts_with("store"))
        .count()
}

#[test]
fn a_side_exit_stores_no_dead_local() {
    // Counter-factual: an exit that stores every local stores 32 locals, two
    // words each, at each of the 16 sites, and the spill code grows with
    // sites times locals however few of them the frame reads again.
    let blocks = 8;
    assert_eq!(stores(&chain(blocks, 0, 32)), stores(&chain(blocks, 0, 0)));
}

#[test]
fn each_live_local_costs_a_side_exit_the_same_stores() {
    // Counter-factual: an exit that stored no local passes the test above,
    // and parks a frame whose resumed body reads nil for every local.
    let blocks = 8;
    let none = stores(&chain(blocks, 0, 0));
    let one = stores(&chain(blocks, 1, 0)) - none;
    assert!(one > 0, "a live local is stored at its sites");
    assert_eq!(stores(&chain(blocks, 4, 0)) - none, 4 * one);
}
