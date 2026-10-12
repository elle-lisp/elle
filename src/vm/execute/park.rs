// audited: 2026-10-06
//! The park an activation's error exit builds, when nothing inside it parked a frame already.
//!
//! docs/impl/vm.md
//! docs/impl/region/park.md

use super::ExecResult;
use crate::value::{BytecodeFrame, SuspendedFrame, Value, SIG_ERROR, SIG_HALT};
use crate::vm::core::VM;

impl VM {
    /// Park the activation `result` describes, which ended on an error, as the
    /// innermost frame of the fiber's chain, and record its raise site in the
    /// delivery ledger.
    ///
    /// An error unwinds nothing (docs/impl/region/unwind.md), so the frame that
    /// raised parks with its locals and operands, and each caller parks behind
    /// it as for a suspend. The frame that parks first is the innermost, and it
    /// is the one the restart delivers into, so it alone records the site. A
    /// raise inside this activation's own callee, or a denial that parked the
    /// frame in call position, left the chain built already, and then this
    /// parks nothing and records nothing.
    ///
    /// A frame a squelch boundary refused at its tail call is ended, not
    /// parked: the chain starts empty, and the restart answers the caller's call.
    /// The callers are `complete_call`, for an interpreted callee, and
    /// `execute_bytecode_saving_stack`, for a body run from Rust, which leaves the
    /// park to its host.
    pub(crate) fn park_raised(&mut self, result: &mut ExecResult) {
        debug_assert!(
            result.bits.intersects(SIG_ERROR) && !result.bits.intersects(SIG_HALT),
            "VM bug: park_raised on {} — only an error parks this way",
            result.bits,
        );
        if self.fiber.suspended.is_some() {
            return;
        }
        let payload = self.fiber.signal.map_or(Value::NIL, |(_, v)| v);
        self.fiber.delivery.park_error(result.site, payload);
        if !result.site.parks_frame() {
            self.fiber.suspended = Some(Vec::new());
            return;
        }
        let frame = BytecodeFrame::suspend(
            result.code.clone(),
            result.env.clone(),
            result.ip,
            std::mem::take(&mut result.stack),
            result.site.delivers(),
            std::mem::take(&mut result.activation_region_map),
            std::mem::take(&mut result.activation_dues),
            result.current_closure,
            self.heap(),
        );
        self.fiber.suspended = Some(vec![SuspendedFrame::Bytecode(frame)]);
    }
}
