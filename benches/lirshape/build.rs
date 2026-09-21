// audited: 2026-09-21
//! The encoder: one `LirFunction` into one region, through a reused buffer.
//!
//! docs/impl/image/measurements.md
//!
//! The shipped region code builds the same way — fill a Rust-side buffer, then
//! copy it into the region in one call (`SyntaxArena::nodes`). Reusing the
//! buffers is what keeps a build's allocator traffic to the region itself.

use elle::hir::region::RuntimeRegion;
use elle::lir::{
    for_each_def, for_each_use, BasicBlock, LirConst, LirFunction, LirInstr, SpannedInstr,
    Terminator,
};
use elle::syntax::Span;
use elle::value::fiberheap::FiberHeap;
use elle::value::region_slice::{RegionSlice, RegionStr};
use elle::value::ConstTemplate;

use crate::node::{PBlock, PConst, PFunc, PInstr, PSite, PTemplate, NO_REG};
use crate::opcode::opcode;

/// The scratch a build reuses across functions, beside the region it fills.
///
/// The shipped region code builds the same way: fill a Rust-side buffer, then
/// copy it into the region in one call (`SyntaxArena::nodes`). Reusing the
/// buffers is what keeps a build's allocator traffic to the region itself.
pub struct Builder {
    heap: *mut FiberHeap,
    region: RuntimeRegion,
    instrs: Vec<PInstr>,
    blocks: Vec<PBlock>,
    consts: Vec<PConst>,
    pool: Vec<u32>,
    templates: Vec<PTemplate>,
    uses: Vec<u32>,
    scratch: Vec<u32>,
    sites: Vec<PSite>,
}

impl Builder {
    pub fn new(heap: &mut FiberHeap, region: RuntimeRegion) -> Self {
        Builder {
            heap: heap as *mut FiberHeap,
            region,
            instrs: Vec::new(),
            blocks: Vec::new(),
            consts: Vec::new(),
            pool: Vec::new(),
            templates: Vec::new(),
            uses: Vec::new(),
            scratch: Vec::new(),
            sites: Vec::new(),
        }
    }

    /// Point the builder at a fresh region, keeping the scratch buffers.
    pub fn retarget(&mut self, region: RuntimeRegion) {
        self.region = region;
    }

    fn slice<T: Copy + 'static>(&mut self, items: &[T]) -> RegionSlice<T> {
        if items.is_empty() {
            return RegionSlice::empty();
        }
        unsafe { (*self.heap).alloc_region_slice_in_region(items, self.region) }
    }

    fn text(&mut self, s: &str) -> RegionStr {
        if s.is_empty() {
            return RegionStr::empty();
        }
        let bytes = self.slice(s.as_bytes());
        unsafe { RegionStr::from_utf8_slice(bytes) }
    }

    /// Copy an already-encoded function into this builder's region — the
    /// deep copy `prepare_task` makes when a hot function goes to the JIT
    /// worker. Every slice is copied, so the result aliases nothing.
    pub fn copy(&mut self, f: &PFunc) -> PFunc {
        self.blocks.clear();
        for b in f.blocks.iter() {
            let instrs = self.slice(b.instrs.as_slice());
            let mut nb = *b;
            nb.instrs = instrs;
            self.blocks.push(nb);
        }
        let built = std::mem::take(&mut self.blocks);
        let blocks = self.slice(&built);
        self.blocks = built;
        PFunc {
            blocks,
            consts: self.slice(f.consts.as_slice()),
            pool: self.slice(f.pool.as_slice()),
            templates: self.slice(f.templates.as_slice()),
            capture_locals: self.slice(f.capture_locals.as_slice()),
            region_table: self.slice(f.region_table.as_slice()),
            merged_slots: self.slice(f.merged_slots.as_slice()),
            frame_release_slots: self.slice(f.frame_release_slots.as_slice()),
            frame_release_regions: self.slice(f.frame_release_regions.as_slice()),
            yield_points: self.slice(f.yield_points.as_slice()),
            call_sites: self.slice(f.call_sites.as_slice()),
            name: {
                let bytes = self.slice(f.name.bytes().as_slice());
                unsafe { RegionStr::from_utf8_slice(bytes) }
            },
            ..*f
        }
    }

    /// Encode `f` into this builder's region.
    pub fn func(&mut self, f: &LirFunction) -> PFunc {
        self.consts.clear();
        self.pool.clear();
        self.templates.clear();
        self.blocks.clear();

        for block in &f.blocks {
            let pb = self.block(block);
            self.blocks.push(pb);
        }
        let blocks = {
            let built = std::mem::take(&mut self.blocks);
            let s = self.slice(&built);
            self.blocks = built;
            s
        };
        let consts = {
            let built = std::mem::take(&mut self.consts);
            let s = self.slice(&built);
            self.consts = built;
            s
        };
        let pool = {
            let built = std::mem::take(&mut self.pool);
            let s = self.slice(&built);
            self.pool = built;
            s
        };
        let templates = {
            let built = std::mem::take(&mut self.templates);
            let s = self.slice(&built);
            self.templates = built;
            s
        };

        let capture_locals = self.slice(f.capture_locals_mask.words());
        let region_table = self.slots(&f.region_table);
        let merged_slots = self.slots(&f.merged_slots);
        let frame_release_slots = self.slice(&f.frame_release_slots);
        let frame_release_regions = self.slots(&f.frame_release_regions);
        let yield_points = self.yields(f);
        let call_sites = self.calls(f);
        let name = match &f.name {
            Some(n) => self.text(n),
            None => RegionStr::empty(),
        };
        let (arity_kind, arity_n) = arity_of(&f.arity);

        PFunc {
            blocks,
            consts,
            pool,
            templates,
            capture_locals,
            region_table,
            merged_slots,
            frame_release_slots,
            frame_release_regions,
            yield_points,
            call_sites,
            name,
            span: f.origin.unwrap_or_else(Span::synthetic),
            closure_id: f.closure_id.map(|c| c.0).unwrap_or(NO_REG),
            entry: f.entry.0,
            num_regs: f.num_regs,
            num_params: f.num_params as u32,
            num_local_params: f.num_local_params as u32,
            arity_kind,
            arity_n,
            vararg_kind: matches!(f.vararg_kind, elle::hir::VarargKind::Struct) as u8,
            num_locals: f.num_locals,
            num_captures: f.num_captures,
            capture_params_mask: f.capture_params_mask,
            signal_bits: f.signal.bits.raw(),
            signal_propagates: f.signal.propagates,
        }
    }

    /// A slice of static-region slots, through the reusable scratch buffer.
    fn slots(&mut self, slots: &[elle::hir::region::StaticRegion]) -> RegionSlice<u32> {
        self.scratch.clear();
        self.scratch.extend(slots.iter().map(|r| r.get()));
        let built = std::mem::take(&mut self.scratch);
        let s = self.slice(&built);
        self.scratch = built;
        s
    }

    fn yields(&mut self, f: &LirFunction) -> RegionSlice<PSite> {
        self.sites.clear();
        for y in &f.yield_points {
            let at = self.pool.len() as u32;
            self.pool.extend(y.stack_regs.iter().map(|r| r.0));
            self.sites.push(PSite {
                resume_ip: y.resume_ip as u32,
                num_locals: y.num_locals,
                regs: at,
                n_regs: y.stack_regs.len() as u32,
            });
        }
        let built = std::mem::take(&mut self.sites);
        let s = self.slice(&built);
        self.sites = built;
        s
    }

    fn calls(&mut self, f: &LirFunction) -> RegionSlice<PSite> {
        self.sites.clear();
        for c in &f.call_sites {
            let at = self.pool.len() as u32;
            self.pool.extend(c.stack_regs.iter().map(|r| r.0));
            self.sites.push(PSite {
                resume_ip: c.resume_ip as u32,
                num_locals: c.num_locals,
                regs: at,
                n_regs: c.stack_regs.len() as u32,
            });
        }
        let built = std::mem::take(&mut self.sites);
        let s = self.slice(&built);
        self.sites = built;
        s
    }

    fn block(&mut self, b: &BasicBlock) -> PBlock {
        self.instrs.clear();
        for si in &b.instructions {
            let pi = self.instr(si);
            self.instrs.push(pi);
        }
        let instrs = {
            let built = std::mem::take(&mut self.instrs);
            let s = self.slice(&built);
            self.instrs = built;
            s
        };
        let (term_op, term_a, term_b, term_c) = self.terminator(&b.terminator.terminator);
        PBlock {
            instrs,
            span: b.terminator.span,
            label: b.label.0,
            term_a,
            term_b,
            term_c,
            term_op,
        }
    }

    fn terminator(&mut self, t: &Terminator) -> (u8, u32, u32, u32) {
        match t {
            Terminator::Return(r) => (0, r.0, NO_REG, NO_REG),
            Terminator::Jump(l) => (1, l.0, NO_REG, NO_REG),
            Terminator::Branch {
                cond,
                then_label,
                else_label,
            } => (2, cond.0, then_label.0, else_label.0),
            Terminator::Emit {
                signal,
                value,
                resume_label,
            } => {
                let at = self.push_const(PConst::Bits(signal.raw()));
                (3, value.0, resume_label.0, at)
            }
            Terminator::Unreachable => (4, NO_REG, NO_REG, NO_REG),
        }
    }

    fn push_const(&mut self, c: PConst) -> u32 {
        self.consts.push(c);
        (self.consts.len() - 1) as u32
    }

    fn instr(&mut self, si: &SpannedInstr) -> PInstr {
        let i = &si.instr;
        let mut dst = NO_REG;
        for_each_def(i, |r| dst = r.0);
        self.uses.clear();
        for_each_use(i, |r| self.uses.push(r.0));
        let n_uses = self.uses.len() as u16;
        let use0 = self.uses.first().copied().unwrap_or(NO_REG);
        let use1 = self.uses.get(1).copied().unwrap_or(NO_REG);
        let extra = self.pool.len() as u32;
        if self.uses.len() > 2 {
            self.pool.extend_from_slice(&self.uses[2..]);
        }
        let (aux, flags) = self.scalars(i);
        PInstr {
            span: si.span,
            dst,
            region: i.region().map(|r| r.get()).unwrap_or(0),
            aux,
            use0,
            use1,
            extra,
            n_uses,
            op: opcode(i),
            flags,
        }
    }

    /// The scalars a variant carries beside its registers: the `aux` word and
    /// the flag bits, with anything wider appended to the operand pool.
    fn scalars(&mut self, i: &LirInstr) -> (u32, u8) {
        use LirInstr::*;
        match i {
            Const { value, .. } | StructGetOrNil { key: value, .. } => {
                let c = self.lir_const(value);
                (self.push_const(c), 0)
            }
            StructGetDestructure { key, .. } => {
                let c = self.lir_const(key);
                (self.push_const(c), 0)
            }
            StructRest { exclude_keys, .. } => {
                let first = self.consts.len() as u32;
                for k in exclude_keys {
                    let c = self.lir_const(k);
                    self.push_const(c);
                }
                self.pool.push(exclude_keys.len() as u32);
                (first, 0)
            }
            ValueConst { value, .. } => (self.push_const(PConst::Val(*value)), 0),
            MaterializeConst { template, .. } => {
                let root = self.template(template);
                (self.push_const(PConst::Template(root)), 0)
            }
            LoadLocal { slot, .. }
            | StoreLocal { slot, .. }
            | StoreLocalRefcounted { slot, .. } => (*slot as u32, 0),
            LoadCapture { index, .. }
            | LoadCaptureRaw { index, .. }
            | StoreCapture { index, .. } => (*index as u32, 0),
            ArrayMutRefDestructure { index, .. }
            | ArrayMutSliceFrom { index, .. }
            | ArrayMutRefOrNil { index, .. } => (*index as u32, 0),
            MakeClosure { closure_id, .. } => (closure_id.0, 0),
            MakeCaptureCell { name, mutated, .. } => {
                (self.push_const(PConst::Bits(name.0)), *mutated as u8)
            }
            CheckSignalBound { allowed_bits, .. } => {
                (self.push_const(PConst::Bits(allowed_bits.raw())), 0)
            }
            BinOp { op, proof, .. } => (0, (*op as u8) | ((proof.is_int() as u8) << 4)),
            Compare { op, proof, .. } => (0, (*op as u8) | ((proof.is_int() as u8) << 4)),
            UnaryOp { op, proof, .. } => (0, (*op as u8) | ((proof.is_int() as u8) << 4)),
            Convert { op, .. } => (0, *op as u8),
            Call { arity_checked, .. } | SuspendingCall { arity_checked, .. } => {
                (0, *arity_checked as u8)
            }
            TailCall {
                arity_checked,
                defer_callee_release,
                deferred_release_slot,
                borrowed_arg_slots,
                ..
            } => {
                self.pool
                    .push(deferred_release_slot.map(|r| r.get()).unwrap_or(0));
                self.pool.push(borrowed_arg_slots.len() as u32);
                for s in borrowed_arg_slots {
                    self.pool.push(*s as u32);
                }
                (
                    0,
                    (*arity_checked as u8) | ((*defer_callee_release as u8) << 1),
                )
            }
            CallArrayMut { args_region, .. } | TailCallArrayMut { args_region, .. } => {
                self.pool.push(args_region.get());
                (0, 0)
            }
            IncrefRegion { region_id } | DecrefRegion { region_id } => (region_id.get(), 0),
            AssertRegionMatches { region_id, .. } => (region_id.get(), 0),
            _ => (0, 0),
        }
    }

    fn lir_const(&mut self, c: &LirConst) -> PConst {
        match c {
            LirConst::Nil => PConst::Nil,
            LirConst::EmptyList => PConst::EmptyList,
            LirConst::Bool(b) => PConst::Bool(*b),
            LirConst::Int(n) => PConst::Int(*n),
            LirConst::Float(f) => PConst::Float(*f),
            LirConst::String(s) => {
                let t = self.text(s);
                PConst::Str(t)
            }
            LirConst::Symbol(s) => PConst::Symbol(s.0),
            LirConst::Keyword(k) => PConst::Keyword(*k),
            LirConst::ClosureRef(i) => PConst::ClosureRef(*i as u32),
            LirConst::ValueRef(i) => PConst::ValueRef(*i as u32),
        }
    }

    /// Flatten a `ConstTemplate` tree into the region, returning its root
    /// index. Children are written first, so a parent names indices that
    /// already exist.
    fn template(&mut self, t: &ConstTemplate) -> u32 {
        let node = match t {
            ConstTemplate::Nil => self.leaf(0, 0, ""),
            ConstTemplate::EmptyList => self.leaf(1, 0, ""),
            ConstTemplate::Bool(b) => self.leaf(2, *b as u64, ""),
            ConstTemplate::Int(n) => self.leaf(3, *n as u64, ""),
            ConstTemplate::Float(f) => self.leaf(4, f.to_bits(), ""),
            ConstTemplate::Symbol(s) => self.leaf(5, 0, s),
            ConstTemplate::Keyword(s) => self.leaf(6, 0, s),
            ConstTemplate::String(s) => self.leaf(7, 0, s),
            ConstTemplate::StringMut(s) => self.leaf(8, 0, s),
            ConstTemplate::Pair(a, b) => {
                let ka = self.template(a);
                let kb = self.template(b);
                let at = self.pool.len() as u32;
                self.pool.push(ka);
                self.pool.push(kb);
                self.node(9, 0, RegionStr::empty(), at, 2, Span::synthetic())
            }
            ConstTemplate::Array(items) | ConstTemplate::ArrayMut(items) => {
                let kids: Vec<u32> = items.iter().map(|i| self.template(i)).collect();
                let at = self.pool.len() as u32;
                self.pool.extend_from_slice(&kids);
                let kind = if matches!(t, ConstTemplate::Array(_)) {
                    10
                } else {
                    11
                };
                self.node(
                    kind,
                    0,
                    RegionStr::empty(),
                    at,
                    kids.len() as u32,
                    Span::synthetic(),
                )
            }
            ConstTemplate::SyntaxSymbol {
                name,
                scopes,
                span,
                scope_exempt,
            } => {
                let text = self.text(name);
                let at = self.pool.len() as u32;
                self.pool.extend_from_slice(scopes);
                let idx = self.node(12, 0, text, at, scopes.len() as u32, *span);
                self.templates[idx as usize].flag = *scope_exempt;
                idx
            }
        };
        node
    }

    fn leaf(&mut self, kind: u8, bits: u64, text: &str) -> u32 {
        let text = self.text(text);
        self.node(kind, bits, text, 0, 0, Span::synthetic())
    }

    fn node(
        &mut self,
        kind: u8,
        bits: u64,
        text: RegionStr,
        kids: u32,
        n_kids: u32,
        span: Span,
    ) -> u32 {
        self.templates.push(PTemplate {
            kind,
            flag: false,
            bits,
            text,
            kids,
            n_kids,
            span,
        });
        (self.templates.len() - 1) as u32
    }
}

fn arity_of(a: &elle::value::Arity) -> (u8, u32) {
    match a {
        elle::value::Arity::Exact(n) => (0, *n as u32),
        elle::value::Arity::AtLeast(n) => (1, *n as u32),
        elle::value::Arity::Range(lo, hi) => (2, ((*lo as u32) << 16) | (*hi as u32)),
    }
}
