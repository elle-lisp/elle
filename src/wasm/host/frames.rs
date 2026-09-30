// audited: 2026-09-29
//! The suspension frames a WASM closure saves when it yields, kept per fiber in the host.
//!
//! docs/impl/wasm.md

use super::ElleHost;
use crate::value::fiber::SignalBits;
use crate::value::Value;

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

impl ElleHost {
    /// Push a suspension frame for the current fiber (appends to back).
    pub fn push_suspension_frame(&mut self, frame: WasmSuspensionFrame) {
        let id = self.current_fiber_id();
        self.suspension_frames
            .entry(id)
            .or_default()
            .push_back(frame);
    }

    /// Pop the front suspension frame for the current fiber (innermost first).
    pub fn pop_suspension_frame(&mut self) -> Option<WasmSuspensionFrame> {
        let id = self.current_fiber_id();
        let frames = self.suspension_frames.get_mut(&id)?;
        let frame = frames.pop_front();
        if frames.is_empty() {
            self.suspension_frames.remove(&id);
        }
        frame
    }

    /// Get the front suspension frame for the current fiber (innermost).
    pub fn first_suspension_frame(&self) -> Option<&WasmSuspensionFrame> {
        let id = self.current_fiber_id();
        self.suspension_frames.get(&id)?.front()
    }

    /// Get the front suspension frame for the current fiber (innermost, mutable).
    pub fn first_suspension_frame_mut(&mut self) -> Option<&mut WasmSuspensionFrame> {
        let id = self.current_fiber_id();
        self.suspension_frames.get_mut(&id)?.front_mut()
    }

    /// The signal on the most recently pushed frame for the current fiber.
    ///
    /// `handle_wasm_result` classifies an `Emit` with this. An `Emit` terminator
    /// carries whatever the emission raised — `(yield v)` and `(error …)` alike
    /// route through `rt_yield` — so "the function returned a non-zero status"
    /// does not by itself mean it parked. This is the back frame rather than the
    /// front because the front may still be a stale outer frame from a previous
    /// suspension until `drive_resume_chain` rotates.
    pub fn back_suspension_frame_signal(&self) -> Option<SignalBits> {
        let id = self.current_fiber_id();
        self.suspension_frames
            .get(&id)?
            .back()
            .map(|f| SignalBits::new(f.signal_bits))
    }

    /// Get the back suspension frame for the current fiber (most recently pushed).
    /// Used by handle_wasm_result to update the frame that rt_yield just pushed.
    pub fn back_suspension_frame_mut(&mut self) -> Option<&mut WasmSuspensionFrame> {
        let id = self.current_fiber_id();
        self.suspension_frames.get_mut(&id)?.back_mut()
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
