// audited: 2026-10-06
//! The frames a fiber parks on this tier, kept per fiber in the host: a
//! compiled closure's saved state, and a bytecode callee's parked chain.
//!
//! docs/impl/wasm.md

use super::ElleHost;
use crate::value::fiber::SignalBits;
use crate::value::{SuspendedFrame, Value};

/// Saved state for a suspended WASM closure.
///
/// When a WASM closure yields (or a callee yields through it), the live
/// registers and env snapshot are saved here. On resume, the env is
/// restored to linear memory and the function is re-invoked with
/// `ctx = resume_state`.
pub struct WasmSuspensionFrame {
    /// Table index of the WASM function to re-invoke.
    pub wasm_func_idx: u32,
    /// Resume state ID (passed as `ctx` parameter on re-entry).
    pub resume_state: u32,
    /// Saved registers at the yield/call point: (tag, payload) pairs.
    pub saved_regs: Vec<(i64, i64)>,
    /// Snapshot of the env region in linear memory. Copied because the
    /// env stack allocator would reclaim the space on return.
    pub env_snapshot: Vec<u8>,
    /// Base address where env_snapshot was taken from (for restore).
    pub env_base: usize,
    /// Full signal bits at the yield point. Preserves SIG_IO and other
    /// bits so the scheduler can detect I/O requests on the fiber.
    pub signal_bits: u64,
    /// The executing closure (`SELF_SLOT`) at the yield point — (tag, payload).
    /// `rt_yield` snapshots it from linear memory; `resume_wasm_closure` writes it
    /// back before re-invoking, so a `LoadSelf` after resume names the closure that
    /// suspended (not whichever ran most recently on this store's shared memory).
    pub self_tag: i64,
    pub self_payload: i64,
    /// A child fiber this frame must RE-DRIVE before resuming its own
    /// continuation. Set when a `(fiber/resume child)` in this frame's body
    /// suspended because `child` emitted a scheduler wait/io its narrow mask does
    /// not cover: the resumer parks holding that wait and the scheduler drives it,
    /// so on resume the scheduler's value must feed a re-drive of `child` — not
    /// this frame's continuation — until `child` completes. The WASM analogue of
    /// the VM's `SuspendedFrame::FiberResume` (src/vm/fiber/trampoline.rs). Pinned
    /// by tests/lang/wasm-protect-suspend.lisp.
    pub redrive_child: Option<Value>,
}

/// The chain a bytecode callee parked, lifted off the driving VM's fiber.
///
/// A compiled call reaches a closure the module never compiled through the
/// host VM (`linker::run_bytecode_closure`), on the VM's own fiber. A park
/// there leaves the callee's frames and the `parameterize` frames it pushed on
/// that fiber, which is not the fiber this tier drives. The bridge moves both
/// here, so the chain resumes with the fiber that parked it, innermost first,
/// before the compiled caller parked behind it (`linker::resume_bytecode_park`).
pub struct BytecodePark {
    /// The callee's frames, innermost first, as `VM::resume_suspended` takes them.
    pub frames: Vec<SuspendedFrame>,
    /// The `parameterize` frames the callee pushed and had not popped.
    pub param_frames: Vec<Vec<(u32, Value)>>,
    /// Whether the resume value owes the `ResumeDelivery` mint that the
    /// delivery ledger recorded for the park (`Delivery::take_resume_funding`).
    pub unfunded: bool,
    /// The signal the chain parked on.
    pub signal: SignalBits,
}

impl BytecodePark {
    /// End a chain that will never resume: run what each frame owed, through
    /// the VM's chokepoint for discarded frames, leaving `payload` standing.
    fn discard(self, heap: &mut crate::value::fiberheap::FiberHeap, payload: Value) {
        let dues = crate::value::fiber::ParkedDues::of(self.frames);
        crate::vm::fiber::release_parked_dues(heap, dues, Some(payload), None);
    }
}

/// One frame of a fiber's parked chain on this tier.
pub enum ParkedFrame {
    /// A compiled closure's frame, saved by `rt_yield`.
    Wasm(WasmSuspensionFrame),
    /// A bytecode callee's chain, lifted off the VM's fiber.
    Bytecode(BytecodePark),
}

impl ParkedFrame {
    fn signal(&self) -> SignalBits {
        match self {
            ParkedFrame::Wasm(f) => SignalBits::new(f.signal_bits),
            ParkedFrame::Bytecode(p) => p.signal,
        }
    }

    fn as_wasm(&self) -> Option<&WasmSuspensionFrame> {
        match self {
            ParkedFrame::Wasm(f) => Some(f),
            ParkedFrame::Bytecode(_) => None,
        }
    }

    fn as_wasm_mut(&mut self) -> Option<&mut WasmSuspensionFrame> {
        match self {
            ParkedFrame::Wasm(f) => Some(f),
            ParkedFrame::Bytecode(_) => None,
        }
    }
}

impl ElleHost {
    /// Push a frame onto the current fiber's chain (appends to back).
    pub fn push_parked(&mut self, frame: ParkedFrame) {
        let id = self.current_fiber_id();
        self.suspension_frames
            .entry(id)
            .or_default()
            .push_back(frame);
    }

    /// Pop the front frame of the current fiber's chain (innermost first).
    pub fn pop_parked(&mut self) -> Option<ParkedFrame> {
        let id = self.current_fiber_id();
        let frames = self.suspension_frames.get_mut(&id)?;
        let frame = frames.pop_front();
        if frames.is_empty() {
            self.suspension_frames.remove(&id);
        }
        frame
    }

    /// Pop the front frame of the current fiber's chain when it is a bytecode
    /// callee's chain.
    pub fn pop_bytecode_park(&mut self) -> Option<BytecodePark> {
        let id = self.current_fiber_id();
        let front = self.suspension_frames.get(&id)?.front()?;
        if front.as_wasm().is_some() {
            return None;
        }
        match self.pop_parked() {
            Some(ParkedFrame::Bytecode(park)) => Some(park),
            _ => unreachable!("the front frame was a bytecode park"),
        }
    }

    /// End every frame fiber `id` parked, running what a bytecode chain among
    /// them owed. `payload` is the value that leaves with the discard, whose
    /// region no release here may take. Fiber ids are addresses, so a fiber
    /// that starts anew may find the chain of a dead fiber that never resumed.
    pub fn discard_parked(&mut self, id: usize, payload: Value) {
        let Some(frames) = self.suspension_frames.remove(&id) else {
            return;
        };
        let heap = unsafe { &mut *self.heap_ptr() };
        for frame in frames {
            if let ParkedFrame::Bytecode(park) = frame {
                park.discard(heap, payload);
            }
        }
    }

    /// The front frame of the current fiber's chain (innermost), when it is a
    /// compiled closure's.
    pub fn first_suspension_frame(&self) -> Option<&WasmSuspensionFrame> {
        let id = self.current_fiber_id();
        self.suspension_frames.get(&id)?.front()?.as_wasm()
    }

    /// The front frame of the current fiber's chain (innermost, mutable), when
    /// it is a compiled closure's.
    pub fn first_suspension_frame_mut(&mut self) -> Option<&mut WasmSuspensionFrame> {
        let id = self.current_fiber_id();
        self.suspension_frames
            .get_mut(&id)?
            .front_mut()?
            .as_wasm_mut()
    }

    /// The signal on the most recently pushed frame for the current fiber.
    ///
    /// `handle_wasm_result` classifies a park with this. A function that
    /// returned a non-zero status parked, and the frame it pushed says on
    /// what: a yield, an error, an io request. This is the back frame rather
    /// than the front because the front may still be a stale outer frame from
    /// a previous suspension until `drive_resume_chain` rotates.
    pub fn back_suspension_frame_signal(&self) -> Option<SignalBits> {
        let id = self.current_fiber_id();
        self.suspension_frames
            .get(&id)?
            .back()
            .map(ParkedFrame::signal)
    }

    /// The most recently pushed frame for the current fiber, when it is a
    /// compiled closure's. `handle_wasm_result` updates the frame `rt_yield`
    /// just pushed.
    pub fn back_suspension_frame_mut(&mut self) -> Option<&mut WasmSuspensionFrame> {
        let id = self.current_fiber_id();
        self.suspension_frames
            .get_mut(&id)?
            .back_mut()?
            .as_wasm_mut()
    }

    /// The child fiber the current fiber's FRONT frame must re-drive before
    /// resuming, if any (see `WasmSuspensionFrame::redrive_child`).
    pub fn first_frame_redrive_child(&self) -> Option<Value> {
        self.first_suspension_frame().and_then(|f| f.redrive_child)
    }

    /// Clear the FRONT frame's re-drive marker — called once the child has been
    /// driven to completion, so the frame resumes its own continuation next.
    pub fn clear_first_frame_redrive(&mut self) {
        if let Some(frame) = self.first_suspension_frame_mut() {
            frame.redrive_child = None;
        }
    }

    /// Check if the current fiber has any suspension frames.
    pub fn has_suspension_frames(&self) -> bool {
        let id = self.current_fiber_id();
        self.suspension_frames
            .get(&id)
            .is_some_and(|f| !f.is_empty())
    }

    /// Count suspension frames for the current fiber.
    pub fn suspension_frame_count(&self) -> usize {
        let id = self.current_fiber_id();
        self.suspension_frames
            .get(&id)
            .map(|f| f.len())
            .unwrap_or(0)
    }
}
