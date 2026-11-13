//! audited: 2026-11-12
//! Moving each port's line of writes along: the next write starts when the
//! one ahead leaves the table.
//!
//! A waiting write whose fiber is gone or whose deadline has passed leaves
//! its line with an answer and nothing sent.
//!
//! docs/impl/io/write.md

use super::*;
use crate::io::pending::WaitingWrite;
use std::time::Instant;

impl AsyncBackendInner {
    /// Answer the waiting writes that leave their lines without starting, then
    /// start the next write of every line whose running write has left the
    /// table. `drain_ready` calls this last, after every completion it reaped
    /// has taken its entry out.
    ///
    /// A write that cannot start answers with an error. It never entered the
    /// table, so the loop takes the next write of the same line at once.
    pub(super) fn advance_lines(&mut self) {
        let origin_heap = self.origin_heap;
        for id in self.pending.orphaned_waiting() {
            let answer = PendingTable::orphaned_asker_error(id, origin_heap);
            self.completions.push_back(answer);
        }
        for id in self.pending.expired_waiting(Instant::now()) {
            let birth = crate::io::Birthplace::on(origin_heap);
            let answer = Completion::failed(id, birth, "timeout", "I/O operation timed out");
            self.completions.push_back(answer);
        }
        while let Some(mut write) = self.pending.next_waiting() {
            if let Err(msg) = self.start_waiting(&mut write) {
                let birth = crate::io::Birthplace::on(origin_heap);
                let answer = Completion::failed(write.id, birth, "io-error", msg);
                self.completions.push_back(answer);
            }
            // `write` drops here and lets go of its hold. A write that started
            // filed an entry with a hold of its own first.
        }
    }

    /// Start a write that waited in line, on the port as the write ahead left
    /// it, with the copy its submission took.
    fn start_waiting(&mut self, write: &mut WaitingWrite) -> Result<(), String> {
        let open = write
            .port
            .as_external::<Port>()
            .is_some_and(|p| !p.is_closed());
        if !open {
            return Err("io/submit: port is closed".into());
        }
        AsyncBackend::submit_stream(
            self,
            &PortOp::sending(write.data),
            write.id,
            write.port,
            write.bound,
            write.submitter,
            write.copy.take(),
        )
        .map(|_| ())
    }

    /// `timeout`, cut short by the earliest deadline among the waiting writes.
    /// No completion may come before that deadline passes, so a wait that
    /// blocked past it would answer the write late.
    pub(super) fn wait_bound(&self, timeout: Option<Duration>) -> Option<Duration> {
        let Some(deadline) = self.pending.earliest_waiting_deadline() else {
            return timeout;
        };
        let left = deadline.saturating_duration_since(Instant::now());
        Some(timeout.map_or(left, |t| t.min(left)))
    }
}
