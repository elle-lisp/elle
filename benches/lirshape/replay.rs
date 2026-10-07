// audited: 2026-10-06
//! The lowerer's shape, replayed into `Vec`s and then frozen, against the same
//! shape replayed into `RegionVec`s and then compacted.
//!
//! docs/impl/image/measurements.md
//!
//! Both sides replay the boot corpus in the order the lowerer emits it. A
//! nested lambda's body comes before the `MakeClosure` that builds it, each
//! instruction is one push, and each block ends in a `finish_block`. Every
//! `TailCall` takes both splices of the relocation (src/lir/lower/splice.rs):
//! a release run moved ahead of the call in its open block, and a replica
//! spliced into that block once it has closed. The splices land where the
//! lowerer's do, at a tail call's index, and come to about as many as the
//! lowerer makes over the same corpus.

use std::time::Instant;

use elle::lir::code::{freeze, LirOwned};
use elle::lir::{BasicBlock, LirConst, LirFunction, LirInstr, LirModule, Reg, SpannedInstr};
use elle::runtime::Runtime;
use elle::syntax::Span;
use elle::value::region_slice::RegionSlice;

use crate::build::{Builder, Tables};
use crate::grow::{Arena, RegionVec};
use crate::node::{PBlock, PFunc, PInstr};
use crate::opcode::opcode;

/// The run a relocation splices: the value route `emit_slot_value_release`
/// emits, which is the run the lowerer replicates.
fn release_run() -> Vec<SpannedInstr> {
    let s = Span::synthetic();
    vec![
        SpannedInstr::new(
            LirInstr::LoadLocal {
                dst: Reg(0),
                slot: 0,
            },
            s,
        ),
        SpannedInstr::new(LirInstr::DecrefValueRegion { src: Reg(0) }, s),
        SpannedInstr::new(
            LirInstr::Const {
                dst: Reg(1),
                value: LirConst::Nil,
            },
            s,
        ),
        SpannedInstr::new(
            LirInstr::StoreLocal {
                slot: 0,
                src: Reg(1),
            },
            s,
        ),
    ]
}

fn is_tail_call(i: &LirInstr) -> bool {
    matches!(
        i,
        LirInstr::TailCall { .. } | LirInstr::TailCallArrayMut { .. }
    )
}

// ── the shipped shape ─────────────────────────────────────────────

/// A `Vec` per block and a push per instruction, spliced as `splice.rs`
/// splices: drain the run into its own vector, then `Vec::splice` it in.
struct VecReplay<'a> {
    run: &'a [SpannedInstr],
    out: Vec<LirFunction>,
}

impl VecReplay<'_> {
    fn module(&mut self, m: &LirModule) {
        let f = self.func(m, &m.entry);
        self.out.push(f);
    }

    fn func(&mut self, m: &LirModule, f: &LirFunction) -> LirFunction {
        let mut nf = crate::ops::shell(f);
        let mut replica: Option<(usize, usize)> = None;
        for b in &f.blocks {
            let mut cur = BasicBlock::new(b.label);
            if let Some((closed, at)) = replica.take() {
                let start = cur.instructions.len();
                for si in self.run {
                    cur.instructions.push(si.clone());
                }
                let copy: Vec<_> = cur.instructions.drain(start..).collect();
                nf.blocks[closed].instructions.splice(at..at, copy);
            }
            let mut tail = None;
            for si in &b.instructions {
                if let LirInstr::MakeClosure { closure_id, .. } = &si.instr {
                    let inner = self.func(m, &m.closures[closure_id.0 as usize]);
                    self.out.push(inner);
                }
                if is_tail_call(&si.instr) {
                    tail = Some(cur.instructions.len());
                }
                cur.instructions.push(si.clone());
            }
            if let Some(at) = tail {
                let start = cur.instructions.len();
                for si in self.run {
                    cur.instructions.push(si.clone());
                }
                let moved: Vec<_> = cur.instructions.drain(start..).collect();
                cur.instructions.splice(at..at, moved);
                replica = Some((nf.blocks.len(), at + self.run.len()));
            }
            cur.terminator = b.terminator.clone();
            nf.blocks.push(cur);
        }
        nf
    }
}

// ── the prototype ─────────────────────────────────────────────────

/// One block of the region working form: its nodes still growable, its
/// terminator already encoded.
#[derive(Clone, Copy)]
struct WBlock {
    nodes: RegionVec<PInstr>,
    span: Span,
    label: u32,
    term: (u8, u32, u32, u32),
}

/// A `RegionVec` per block and a push per node, spliced through one reused
/// scratch buffer, and each function compacted into exact slices as it ends.
struct RegionReplay<'a> {
    b: &'a mut Builder,
    arena: *mut Arena,
    run: &'a [SpannedInstr],
    spares: Vec<Tables>,
    scratch: Vec<PInstr>,
    out: Vec<PFunc>,
}

impl RegionReplay<'_> {
    fn module(&mut self, m: &LirModule) {
        let f = self.func(m, &m.entry);
        self.out.push(f);
    }

    /// Push the run onto `cur`, then move it out into the scratch buffer, as
    /// the lowerer emits a run at the end of the open block and then takes it.
    fn take_run(&mut self, cur: &mut RegionVec<PInstr>) {
        let start = cur.len();
        for si in self.run {
            let n = self.b.instr(si);
            cur.push(n);
        }
        self.scratch.clear();
        self.scratch.extend_from_slice(&cur.as_slice()[start..]);
        cur.truncate(start);
    }

    fn func(&mut self, m: &LirModule, f: &LirFunction) -> PFunc {
        let spare = self.spares.pop().unwrap_or_default();
        let parent = self.b.swap_tables(spare);
        let mut blocks: RegionVec<WBlock> = RegionVec::new(self.arena);
        let mut replica: Option<(usize, usize)> = None;
        for b in &f.blocks {
            let mut cur: RegionVec<PInstr> = RegionVec::new(self.arena);
            if let Some((closed, at)) = replica.take() {
                self.take_run(&mut cur);
                blocks.as_mut_slice()[closed]
                    .nodes
                    .insert_slice(at, &self.scratch);
            }
            let mut tail = None;
            for si in &b.instructions {
                if let LirInstr::MakeClosure { closure_id, .. } = &si.instr {
                    let inner = self.func(m, &m.closures[closure_id.0 as usize]);
                    self.out.push(inner);
                }
                if is_tail_call(&si.instr) {
                    tail = Some(cur.len());
                }
                let n = self.b.instr(si);
                cur.push(n);
            }
            if let Some(at) = tail {
                self.take_run(&mut cur);
                cur.insert_slice(at, &self.scratch);
                replica = Some((blocks.len(), at + self.run.len()));
            }
            let term = self.b.terminator(&b.terminator.terminator);
            blocks.push(WBlock {
                nodes: cur,
                span: b.terminator.span,
                label: b.label.0,
                term,
            });
        }
        let pf = self.compact(f, blocks);
        let mut mine = self.b.swap_tables(parent);
        mine.clear();
        self.spares.push(mine);
        pf
    }

    /// Copy every block's nodes into one exact extent, as the frozen form
    /// lays them out, and the tables beside them.
    fn compact(&mut self, f: &LirFunction, blocks: RegionVec<WBlock>) -> PFunc {
        let arena = unsafe { &mut *self.arena };
        let total: usize = blocks.as_slice().iter().map(|w| w.nodes.len()).sum();
        let nodes = arena.alloc::<PInstr>(total);
        let recs = arena.alloc::<PBlock>(blocks.len());
        let mut at = 0;
        for (k, w) in blocks.as_slice().iter().enumerate() {
            let n = w.nodes.len();
            unsafe {
                std::ptr::copy_nonoverlapping(w.nodes.as_slice().as_ptr(), nodes.add(at), n);
                recs.add(k).write(PBlock {
                    instrs: RegionSlice::from_raw(nodes.add(at), n as u32),
                    span: w.span,
                    label: w.label,
                    term_a: w.term.1,
                    term_b: w.term.2,
                    term_c: w.term.3,
                    term_op: w.term.0,
                });
            }
            at += n;
        }
        let recs = unsafe { RegionSlice::from_raw(recs, blocks.len() as u32) };
        self.b.finish(f, recs)
    }
}

// ── the check and the report ──────────────────────────────────────

/// Do the two replays hold the same functions, block for block and opcode for
/// opcode? The prototype's splice is only measured once it answers as
/// `Vec::splice` does.
fn agree(shipped: &[LirFunction], proto: &[PFunc]) {
    assert_eq!(
        shipped.len(),
        proto.len(),
        "the replays built different functions"
    );
    for (s, p) in shipped.iter().zip(proto) {
        assert_eq!(s.blocks.len(), p.blocks.len(), "a function lost a block");
        for (sb, pb) in s.blocks.iter().zip(p.blocks.iter()) {
            assert_eq!(sb.label.0, pb.label, "blocks out of order");
            let ops: Vec<u8> = sb.instructions.iter().map(|si| opcode(&si.instr)).collect();
            let pops: Vec<u8> = pb.instrs.iter().map(|n| n.op).collect();
            assert_eq!(ops, pops, "block {} differs between the replays", pb.label);
        }
    }
}

fn nodes(fs: &[LirFunction]) -> usize {
    fs.iter()
        .flat_map(|f| f.blocks.iter())
        .map(|b| b.instructions.len())
        .sum()
}

/// Replay `modules` both ways, check the two agree, and report the costs.
pub fn report(rt: &mut Runtime, modules: &[LirModule]) {
    let run = release_run();
    let tails = modules
        .iter()
        .flat_map(|m| std::iter::once(&m.entry).chain(m.closures.iter()))
        .flat_map(|f| f.blocks.iter())
        .flat_map(|b| b.instructions.iter())
        .filter(|si| is_tail_call(&si.instr))
        .count();

    let first = rt.heap().new_runtime_region();
    let mut builder = Builder::new(rt.heap(), first);
    let mut spares: Vec<Tables> = Vec::new();
    let mut scratch: Vec<PInstr> = Vec::new();

    // One round of each, outside the timer, for the check.
    let mut shipped = VecReplay {
        run: &run,
        out: Vec::new(),
    };
    for m in modules {
        shipped.module(m);
    }
    let pushed = nodes(&shipped.out);
    let r = rt.heap().new_runtime_region();
    let mut arena = Arena::new(rt.heap(), r);
    builder.retarget(r);
    let mut proto = RegionReplay {
        b: &mut builder,
        arena: &mut arena,
        run: &run,
        spares: std::mem::take(&mut spares),
        scratch: std::mem::take(&mut scratch),
        out: Vec::new(),
    };
    for m in modules {
        proto.module(m);
    }
    agree(&shipped.out, &proto.out);
    spares = std::mem::take(&mut proto.spares);
    scratch = std::mem::take(&mut proto.scratch);
    drop(shipped);
    rt.heap().decref_region_if_present(r);

    println!(
        "  replay: {pushed} nodes, {tails} tail calls, {} splices of {} nodes",
        2 * tails,
        run.len()
    );
    println!("  check:  both replays hold the same blocks and opcodes");
    println!();

    let (mut v_build, mut v_freeze, mut v_drop) = (Vec::new(), Vec::new(), Vec::new());
    let (mut v_allocs, mut v_held) = (0, 0);
    for _ in 0..crate::ROUNDS {
        let (a0, _) = crate::allocs();
        let live0 = crate::live_bytes();
        let t0 = Instant::now();
        let mut rep = VecReplay {
            run: &run,
            out: Vec::new(),
        };
        for m in modules {
            rep.module(m);
        }
        let t1 = Instant::now();
        let frozen: Vec<LirOwned> = rep.out.iter().map(|f| freeze(f).unwrap()).collect();
        let t2 = Instant::now();
        v_held = crate::live_bytes() - live0;
        v_allocs = crate::allocs().0 - a0;
        drop(frozen);
        drop(rep);
        let t3 = Instant::now();
        v_build.push((t1 - t0).as_nanos() as f64);
        v_freeze.push((t2 - t1).as_nanos() as f64);
        v_drop.push((t3 - t2).as_nanos() as f64);
    }

    let (mut r_build, mut r_drop) = (Vec::new(), Vec::new());
    let (mut r_allocs, mut r_pages, mut r_abandoned) = (0, 0, 0);
    let claims0 = rt.heap().page_claims();
    for _ in 0..crate::ROUNDS {
        let (a0, _) = crate::allocs();
        let pages0 = rt.heap().allocated_bytes();
        let t0 = Instant::now();
        let mut regions = Vec::with_capacity(modules.len());
        let mut abandoned = 0;
        for m in modules {
            let r = rt.heap().new_runtime_region();
            let mut arena = Arena::new(rt.heap(), r);
            builder.retarget(r);
            let mut rep = RegionReplay {
                b: &mut builder,
                arena: &mut arena,
                run: &run,
                spares: std::mem::take(&mut spares),
                scratch: std::mem::take(&mut scratch),
                out: Vec::new(),
            };
            rep.module(m);
            spares = std::mem::take(&mut rep.spares);
            scratch = std::mem::take(&mut rep.scratch);
            std::hint::black_box(&rep.out);
            abandoned += arena.abandoned;
            regions.push(r);
        }
        let t1 = Instant::now();
        r_pages = rt.heap().allocated_bytes() - pages0;
        r_allocs = crate::allocs().0 - a0;
        r_abandoned = abandoned;
        for r in regions {
            rt.heap().decref_region_if_present(r);
        }
        let t2 = Instant::now();
        r_build.push((t1 - t0).as_nanos() as f64);
        r_drop.push((t2 - t1).as_nanos() as f64);
    }
    let r_claims = rt.heap().page_claims() - claims0;
    rt.heap().decref_region_if_present(first);

    let (vb, vf, vd) = (
        crate::best(&v_build),
        crate::best(&v_freeze),
        crate::best(&v_drop),
    );
    let (rb, rd) = (crate::best(&r_build), crate::best(&r_drop));
    println!("  Vec per block, then freeze");
    crate::report("build (push, finish, splice)", vb, pushed);
    crate::report("freeze", vf, pushed);
    crate::report("teardown", vd, pushed);
    crate::report("total", vb + vf + vd, pushed);
    println!(
        "    {v_allocs} malloc calls per round, {} KiB held while live",
        v_held / 1024
    );
    println!();
    println!("  RegionVec per block, compacted as each function ends");
    crate::report("build and compact", rb, pushed);
    crate::report("teardown (free the regions)", rd, pushed);
    crate::report("total", rb + rd, pushed);
    println!(
        "    {r_allocs} malloc calls per round, {} KiB of region pages, {} KiB of it abandoned by growth",
        r_pages / 1024,
        r_abandoned / 1024,
    );
    println!("    {} page claims over {} rounds", r_claims, crate::ROUNDS);
    println!();
}
