// audited: 2026-11-27
// docs/impl/jit.md
// docs/impl/jit-closure.md
//! The stack slots compiled code hands the runtime: one fresh per register or arena list, and one every side exit shares.
//!
//! A call, an array, a parameter frame and a region group each store their
//! registers in a fresh slot of their own, and a tail call its merged arenas'
//! static slots. The prologue sizes the shared slot once to the worst-case
//! save requirement. A side exit stores the parameters, the locals live where
//! the interpreter resumes, and the operands into it, and records which locals
//! it stored. The shared slot's methods stay `pub(crate)` because the compiler
//! driver allocates it in the prologue, and the terminator and instruction
//! paths spill into it.

use super::*;
use crate::lir::code::{BlockLiveness, LiveSlots, SlotLiveness};

/// One side exit, by its kind and its index among the sites of that kind.
#[derive(Clone, Copy)]
pub(crate) enum Site {
    Yield(u32),
    Call(u32),
    Charge(u32),
}

/// Where the interpreter resumes a side exit, which decides the locals the
/// exit keeps (docs/impl/jit-closure.md).
#[derive(Clone, Copy)]
pub(crate) enum Resumes {
    /// At the node being translated, which runs again.
    AtNode,
    /// Past the node being translated.
    PastNode,
    /// At the start of the block labeled so.
    Into(Label),
}

/// The local slots live where each side exit resumes, and the slots live
/// around each node of the block being translated. A function with no site
/// computes neither.
pub(crate) struct SpillLiveness {
    slots: SlotLiveness,
    block: Option<BlockLiveness>,
    /// The node of the block being translated.
    node: usize,
}

impl SpillLiveness {
    /// The liveness `lir`'s side exits read, or `None` where it has no site.
    pub(crate) fn of(lir: &crate::lir::LirView<'_>) -> Option<Self> {
        let sites = lir.yield_points().len() + lir.call_sites().len() + lir.charge_sites().len();
        (sites != 0).then(|| SpillLiveness {
            slots: SlotLiveness::of(lir),
            block: None,
            node: 0,
        })
    }

    /// The locals live where `at` resumes.
    fn kept(&self, at: Resumes) -> LiveSlots {
        let block = || {
            self.block
                .as_ref()
                .expect("JIT bug: a node spills before its block is entered")
        };
        match at {
            Resumes::AtNode => block().before(self.node).clone(),
            Resumes::PastNode => block().after(self.node).clone(),
            Resumes::Into(label) => self.slots.live_in(label).clone(),
        }
    }
}

/// The locals each side exit of one function stores, by site; `None` for a
/// site in code the translation skips, whose exit never runs.
#[derive(Default)]
pub(crate) struct KeptLocals {
    pub(crate) yields: Vec<Option<LiveSlots>>,
    pub(crate) calls: Vec<Option<LiveSlots>>,
    pub(crate) charges: Vec<Option<LiveSlots>>,
}

impl KeptLocals {
    /// One empty entry per site `lir` records.
    pub(crate) fn of(lir: &crate::lir::LirView<'_>) -> Self {
        KeptLocals {
            yields: vec![None; lir.yield_points().len()],
            calls: vec![None; lir.call_sites().len()],
            charges: vec![None; lir.charge_sites().len()],
        }
    }

    fn record(&mut self, site: Site, kept: LiveSlots) {
        let (sites, index) = match site {
            Site::Yield(i) => (&mut self.yields, i),
            Site::Call(i) => (&mut self.calls, i),
            Site::Charge(i) => (&mut self.charges, i),
        };
        sites[index as usize] = Some(kept);
    }
}

impl<'a> FunctionTranslator<'a> {
    /// Start translating the block at `position`.
    pub(crate) fn enter_block(&mut self, position: usize) {
        self.current_block = position;
        if let Some(live) = self.spill_liveness.as_mut() {
            live.block = Some(live.slots.block(&self.lir, position));
            live.node = 0;
        }
    }

    /// Start translating node `node` of the current block.
    pub(crate) fn enter_node(&mut self, node: usize) {
        if let Some(live) = self.spill_liveness.as_mut() {
            live.node = node;
        }
    }

    /// Store the values of `regs` in a fresh stack slot, one `Value` each, and
    /// answer the slot's address: null where `regs` is empty. Every register
    /// is read before the caller writes any variable, so a list that names a
    /// variable the caller then binds reads its old value.
    pub(crate) fn spill_regs(
        &mut self,
        builder: &mut FunctionBuilder,
        regs: &[Reg],
    ) -> cranelift_codegen::ir::Value {
        if regs.is_empty() {
            return builder.ins().iconst(I64, 0);
        }
        let slot = builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
            (regs.len() * std::mem::size_of::<crate::value::Value>()) as u32,
            0,
        ));
        for (i, reg) in regs.iter().enumerate() {
            let (tag, payload) = self.use_var_pair(builder, reg.0);
            store_value_slot(builder, slot, i as u32, tag, payload);
        }
        builder.ins().stack_addr(I64, slot, 0)
    }

    /// Store a tail call's arena slots in a fresh stack slot, one `u32` each,
    /// and answer the slot's address and the count: null and 0 where the call
    /// strands no arena.
    pub(crate) fn spill_arena_slots(
        &mut self,
        builder: &mut FunctionBuilder,
        arenas: crate::lir::ArenaSlots<'_>,
    ) -> (cranelift_codegen::ir::Value, cranelift_codegen::ir::Value) {
        let count = builder.ins().iconst(I64, arenas.len() as i64);
        if arenas.is_empty() {
            return (builder.ins().iconst(I64, 0), count);
        }
        let slot = builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
            (arenas.len() * std::mem::size_of::<u32>()) as u32,
            2,
        ));
        for (i, &word) in arenas.words().iter().enumerate() {
            let word = builder.ins().iconst(I32, i64::from(word));
            builder.ins().stack_store(I64, word, slot, i as i32 * 4);
        }
        (builder.ins().stack_addr(I64, slot, 0), count)
    }

    /// Allocate the shared spill slot sized to the maximum spill requirement.
    pub(crate) fn allocate_shared_spill_slot(&mut self, builder: &mut FunctionBuilder) {
        let num_locals = self.lir.num_locals() as usize;

        let max_operands = self
            .lir
            .yield_points()
            .chain(self.lir.call_sites())
            .chain(self.lir.charge_sites())
            .map(|site| site.stack_regs.len())
            .max()
            .unwrap_or(0);
        // Spill saves: arity params (arg_vars) + num_locals locals (local_var_base)
        // + operand stack entries.
        let arity = self.lir.num_params();
        let max_total = arity + num_locals + max_operands;

        if max_total > 0 {
            // Each Value is 16 bytes
            let slot = builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
                cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                (max_total * 16) as u32,
                0,
            ));
            self.shared_spill_slot = Some(slot);
        }
    }

    /// Spill `site`'s exit into the shared stack slot: every parameter, each
    /// local live where `at` resumes, then `stack_regs`. A local keeps its
    /// place whether or not the exit stores it, and the slot of one it skips
    /// holds whatever an earlier exit stored there, which `rebuild_frame`
    /// never reads. Records the locals stored against `site`.
    ///
    /// Returns a Cranelift value pointing to the spilled buffer (*const Value),
    /// or a null pointer constant if there's nothing to spill.
    pub(crate) fn spill_locals_and_operands(
        &mut self,
        builder: &mut FunctionBuilder,
        site: Site,
        at: Resumes,
        stack_regs: &[Reg],
    ) -> Result<cranelift_codegen::ir::Value, JitError> {
        let arity = self.lir.num_params() as u32;
        let num_locals = self.lir.num_locals() as u32;
        let kept = self
            .spill_liveness
            .as_ref()
            .expect("JIT bug: a side exit in a function that records no site")
            .kept(at);
        if let Some(l) = kept.iter().find(|&l| l as u32 >= num_locals) {
            return Err(JitError::InvalidLir(format!(
                "local slot {l} is live at a side exit of a frame of {num_locals} locals"
            )));
        }

        if arity + num_locals + stack_regs.len() as u32 == 0 {
            self.kept_locals.record(site, kept);
            return Ok(builder.ins().iconst(I64, 0)); // null pointer
        }

        let slot = self
            .shared_spill_slot
            .expect("JIT bug: spill_locals_and_operands called but no shared spill slot allocated");

        for i in 0..arity {
            let (tag, payload) = self.use_var_pair(builder, self.arg_var_base + i);
            store_value_slot(builder, slot, i, tag, payload);
        }
        for l in kept.iter() {
            let (tag, payload) = self.use_var_pair(builder, self.local_var_base + l as u32);
            store_value_slot(builder, slot, arity + l as u32, tag, payload);
        }
        for (j, reg) in stack_regs.iter().enumerate() {
            let (tag, payload) = self.use_var_pair(builder, reg.0);
            store_value_slot(builder, slot, arity + num_locals + j as u32, tag, payload);
        }
        self.kept_locals.record(site, kept);

        Ok(builder.ins().stack_addr(I64, slot, 0))
    }
}
