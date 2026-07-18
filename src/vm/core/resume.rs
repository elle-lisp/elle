use super::*;

impl VM {
    /// Resume execution from suspended frames.
    ///
    /// Replays the frame chain from innermost (index 0) to outermost
    /// (last index), threading the resume value through. For single-frame
    /// suspension (signal-based), this is equivalent to a simple resume.
    /// For multi-frame suspension (yield-through-calls), this replays the
    /// full call chain.
    ///
    /// Handles two frame types:
    /// - `Bytecode`: restores the saved operand stack and continues bytecode
    ///   execution from the saved instruction pointer.
    /// - `FiberResume`: resumes a suspended sub-fiber (from `defer`/`protect`)
    ///   with the current value via `do_fiber_resume`, using the proper
    ///   fiber-swap machinery so heap context and parent/child chain are correct.
    ///
    /// Returns SignalBits. The result value is stored in `self.fiber.signal`.
    pub fn resume_suspended(
        &mut self,
        frames: Vec<SuspendedFrame>,
        resume_value: Value,
    ) -> SignalBits {
        if frames.is_empty() {
            self.fiber.signal = Some((SIG_OK, resume_value));
            return SIG_OK;
        }

        // Save current stack state
        let saved_stack = std::mem::take(&mut self.fiber.stack);

        let mut current_value = resume_value;

        for i in 0..frames.len() {
            let frame = &frames[i];

            match frame {
                SuspendedFrame::FiberResume {
                    handle,
                    fiber_value,
                } => {
                    // Trampoline: instead of calling do_fiber_resume (which
                    // would recurse on the Rust stack), set pending_fiber_resume
                    // and return SIG_SWITCH. The trampoline in do_fiber_resume
                    // will handle the fiber transition iteratively.
                    handle.with_mut(|f| {
                        f.signal = Some((SIG_OK, current_value));
                    });

                    // The inner fiber's true parent is the fiber whose frames
                    // we are replaying — the currently active one.
                    self.pending_fiber_resume = Some(PendingFiberResume {
                        handle: handle.clone(),
                        fiber_value: *fiber_value,
                        parent: self
                            .current_fiber_handle
                            .clone()
                            .map(|h| (h, self.current_fiber_value)),
                    });

                    // Save remaining outer frames for later resumption.
                    if i + 1 < frames.len() {
                        self.fiber.suspended = Some(frames[i + 1..].to_vec());
                    }

                    self.fiber.signal = Some((SIG_SWITCH, Value::NIL));
                    self.fiber.stack = saved_stack;
                    return SIG_SWITCH;
                }

                SuspendedFrame::Bytecode(frame) => {
                    // Restore this frame's stack
                    self.fiber.stack.clear();
                    self.fiber.stack.extend(frame.stack.iter().copied());

                    // For yield frames and caller frames: the resume value is the
                    // "return value" of the suspended operation (yield result, or
                    // call return). Push it so the next instruction sees it.
                    // For fuel/signal-pause frames: the instruction at frame.ip
                    // re-executes from scratch — no extra value is injected.
                    if frame.push_resume_value {
                        self.fiber.stack.push(current_value);
                    }

                    if self
                        .runtime_config
                        .has_trace_bit(crate::config::trace_bits::CALL)
                    {
                        let opcode = if frame.ip < frame.code.bytecode.len() {
                            frame.code.bytecode[frame.ip]
                        } else {
                            255
                        };
                        let env_ptr = std::rc::Rc::as_ptr(&frame.env) as usize;
                        eprintln!(
                            "[resume] frame={} ip={} bc_len={} opcode={} saved_stack={} push_rv={} final_stack={} env_len={} env_ptr={:#x} rv_type={}",
                            i, frame.ip, frame.code.bytecode.len(), opcode,
                            frame.stack.len(), frame.push_resume_value,
                            self.fiber.stack.len(), frame.env.len(),
                            env_ptr, current_value.type_name(),
                        );
                        for (si, sv) in self.fiber.stack.iter().enumerate() {
                            eprintln!("  stack[{}] = {} {:?}", si, sv.type_name(), sv);
                        }
                        // Only dump env for small envs (inner closures, not stdlib)
                        if frame.env.len() <= 5 {
                            for (ei, ev) in frame.env.iter().enumerate() {
                                let detail = if let Some(inner) = ev.capture_cell_get() {
                                    // Identity = the cell's heap address.
                                    let cell_ptr = ev.as_heap_ptr().map_or(0, |p| p as usize);
                                    format!(
                                        "box(ptr={:#x}) -> {} {:?}",
                                        cell_ptr,
                                        inner.type_name(),
                                        inner
                                    )
                                } else {
                                    format!("{} {:?}", ev.type_name(), ev)
                                };
                                eprintln!("  env[{}] = {}", ei, detail);
                            }
                        }
                    }

                    // Confirm every uncounted region borrow this suspended frame
                    // holds across park/resume is still live (debug builds). The
                    // snapshot (`record_region_borrows`) recorded only the LIVE
                    // mapped regions — the suspended activation's own allocations,
                    // held alive by its still-pending `DecrefRegion`s — each tagged
                    // with the generation the slot was established at, so a dead
                    // leftover (a recycled id the activation no longer owns) is
                    // already excluded. A borrow freed *while the fiber was parked*
                    // would be silently restored below and corrupt the resumed body's
                    // allocs/decrefs. Panic at the boundary, naming the slot, instead
                    // of at a later stale read (docs/impl/region/generations.md
                    // § "Two borrow shapes").
                    #[cfg(debug_assertions)]
                    if let Some((slot, r)) =
                        crate::vm::fiber::first_stale_borrow(&frame.region_borrows, self.heap())
                    {
                        panic!(
                            "stale suspended-frame region borrow on resume: activation \
                             region slot {slot} maps to region {r}, which was freed while \
                             this fiber was parked — an uncounted suspended-frame borrow \
                             outlived its region (docs/impl/region/generations.md \
                             § 'Uncounted-borrow check')"
                        );
                    }

                    // Restore this activation's static→physical region remap
                    // as the current frame before re-entering its body, so
                    // post-resume allocs/decrefs resolve in the same frame the
                    // pre-yield allocations did (docs/impl/region/owner.md). The
                    // parked owner node is restored with it, so the resumed
                    // body's normal completion frees it through the trampoline's
                    // clean break. The chain is
                    // replayed sequentially (not Rust-nested), so each frame
                    // gets its own push/pop around its resumed execution.
                    // `from_ip` does not push/pop, so we manage it here.
                    self.restore_activation_region_map(
                        frame.activation_region_map.clone(),
                        frame.activation_owner_node,
                    );

                    // Re-install the executing-closure register parked at suspend so
                    // a self-edge resolved after resume names the right closure (the
                    // peer of `restore_activation_region_map`). `execute_bytecode_from_ip`
                    // does not bracket it — `resume_suspended` manages it per frame,
                    // exactly as it manages the region-map restore/pop.
                    self.fiber.current_closure = frame.current_closure;

                    let exec = self.execute_bytecode_from_ip(&frame.code, &frame.env, frame.ip);

                    if exec.bits.is_ok() {
                        self.pop_activation_region_map();
                        let (_, v) = self.fiber.signal.take().unwrap();
                        if self
                            .runtime_config
                            .has_trace_bit(crate::config::trace_bits::FIBER)
                        {
                            eprintln!(
                                "[resume_suspended] frame {} OK: val_type={} total_frames={}",
                                i,
                                v.type_name(),
                                frames.len(),
                            );
                        }
                        current_value = v;
                    } else {
                        if self
                            .runtime_config
                            .has_trace_bit(crate::config::trace_bits::FIBER)
                        {
                            let susp_len =
                                self.fiber.suspended.as_ref().map(|v| v.len()).unwrap_or(0);
                            let remaining = frames.len() - i - 1;
                            eprintln!(
                                "[resume_suspended] frame {} non-OK: bits={} susp_frames={} remaining={}",
                                i, exec.bits, susp_len, remaining,
                            );
                        }
                        if !exec.bits.contains(SIG_HALT) && self.fiber.suspended.is_none() {
                            // `from_ip` does not pop, so the activation's remap
                            // (mutated by this resumed execution) is still on
                            // top. Carry it forward to the re-suspend frame,
                            // MOVING the owner node with it (a yield park
                            // already took the node, so this reads `None` there
                            // — the move discipline holds either way).
                            let activation_region_map = self
                                .fiber
                                .activation_region_maps
                                .last()
                                .cloned()
                                .unwrap_or_default();
                            let activation_owner_node = self.take_activation_owner_node();
                            let re_suspend = BytecodeFrame::suspend(
                                exec.code,
                                exec.env,
                                exec.ip,
                                exec.stack,
                                !exec.bits.contains(SIG_FUEL),
                                activation_region_map,
                                activation_owner_node,
                                exec.current_closure,
                                self.heap(),
                            );
                            self.fiber.suspended = Some(vec![SuspendedFrame::Bytecode(re_suspend)]);
                        }

                        // For suspending signals (any bits except error/halt),
                        // merge remaining outer frames
                        if !exec.bits.contains(SIG_ERROR)
                            && !exec.bits.contains(SIG_HALT)
                            && i + 1 < frames.len()
                        {
                            if let Some(ref mut new_frames) = self.fiber.suspended {
                                for f in frames[i + 1..].iter() {
                                    new_frames.push(f.clone());
                                }
                            }
                        }

                        // Balance the push from before re-entry. Any re-suspend
                        // frames built above already cloned the live remap, so
                        // discarding it here is safe.
                        self.pop_activation_region_map();
                        self.fiber.stack = saved_stack;
                        return exec.bits;
                    }
                }
            }
        }

        self.fiber.stack = saved_stack;
        self.fiber.signal = Some((SIG_OK, current_value));
        SIG_OK
    }
}
