use super::*;

/// End-to-end exercise of the ACTIVATION OWNER NODE on the interpreter
/// (docs/impl/region/owner.md § "Owner nodes — an activation as a forest root").
/// The body is hand-emitted bytecode — isolating the runtime node mechanism
/// from the compiler's adopt siting (the production emitters are the
/// capture-back-edge and transfer cuts): load a fresh-region member from the
/// constant pool, adopt it into the current activation's (lazily-minted) owner
/// node, return nil.
/// The activation's NORMAL completion — the trampoline's clean break — must free
/// the node, whose subtree drop reclaims the member: its generation bumps (pages
/// returned) and the live region count stays bounded across 50 activations. The
/// counterfactual is the adopt itself: the member is Owned (its count consumed),
/// so if the completion release does not fire, NOTHING reclaims it — node + member
/// entries survive every run and the count grows by 2 per activation.
#[test]
fn activation_owner_node_frees_adopted_member_on_normal_completion() {
    use crate::compiler::bytecode::{Bytecode, Instruction};
    use std::rc::Rc;

    let mut vm = crate::vm::VM::new();
    let heap_ptr = vm.heap_ptr;
    let baseline = unsafe { &*heap_ptr }.active_region_count();

    for _ in 0..50 {
        // The member: a pair in its own fresh region on the VM's heap.
        let (child, child_rid) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        let gen_before = unsafe { &*heap_ptr }.generation_raw(child_rid.get());

        // Body: push the member, adopt it into the activation node, return nil.
        let mut bc = Bytecode::new();
        let idx = bc.add_constant(child);
        bc.emit(Instruction::LoadConst);
        bc.emit_u16(idx);
        bc.emit(Instruction::AdoptIntoActivation);
        bc.emit(Instruction::Nil);
        bc.emit(Instruction::Return);
        let code = crate::value::Code::new(
            Rc::new(bc.instructions),
            Rc::new(bc.constants),
            Rc::new(crate::error::LocationMap::new()),
            Rc::new(vec![]),
        );

        let result = vm.execute_bytecode_saving_stack(&code, &Rc::new(vec![]));
        assert!(
            result.bits.is_empty(),
            "the adopt-and-return body completes normally"
        );
        let gen_after = unsafe { &*heap_ptr }.generation_raw(child_rid.get());
        assert!(
            gen_after > gen_before,
            "the adopted member's pages must be returned (generation bumped) by \
             the owner node's subtree drop at the activation's normal completion \
             (gen {gen_before} -> {gen_after})",
        );
    }

    let after = unsafe { &*heap_ptr }.active_region_count();
    assert!(
        after <= baseline,
        "node + member must be reclaimed at each activation's completion — live \
         region count must not grow (baseline={baseline}, after 50 activations={after})",
    );
}

/// The activation owner node SURVIVES a yield→resume park
/// (docs/impl/region/owner.md § "Owner nodes" — "A park moves the node into the
/// suspended frame"). The hand-emitted body (no production lowering emits
/// `AdoptIntoActivation`) adopts a fresh-region member into the activation's
/// node, yields, and — once resumed — completes normally. The park must carry
/// the node (the member stays Owned, RC frozen, while the fiber is parked — it
/// must NOT be freed mid-park), and the RESUMED body's normal completion must
/// free node + member: generation bump, bounded region count over 50
/// activations. The counterfactual is the park itself: a suspend that drops the
/// node slot strands the Owned member — nothing ever reclaims it, the
/// generation never bumps, and the count grows by 2 per activation.
#[test]
fn activation_owner_node_survives_yield_resume_completion() {
    use crate::compiler::bytecode::{Bytecode, Instruction};
    use std::rc::Rc;

    let mut vm = crate::vm::VM::new();
    let heap_ptr = vm.heap_ptr;
    let baseline = unsafe { &*heap_ptr }.active_region_count();

    for _ in 0..50 {
        let (child, child_rid) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        let gen_before = unsafe { &*heap_ptr }.generation_raw(child_rid.get());

        // Body: adopt the member, yield nil, then (on resume) return the
        // resume value pushed as the yield expression's result.
        let mut bc = Bytecode::new();
        let idx = bc.add_constant(child);
        bc.emit(Instruction::LoadConst);
        bc.emit_u16(idx);
        bc.emit(Instruction::AdoptIntoActivation);
        bc.emit(Instruction::Nil);
        bc.emit(Instruction::Emit);
        bc.emit_signal_bits(crate::value::fiber::SIG_YIELD);
        bc.emit(Instruction::Return);
        let code = crate::value::Code::new(
            Rc::new(bc.instructions),
            Rc::new(bc.constants),
            Rc::new(crate::error::LocationMap::new()),
            Rc::new(vec![]),
        );

        let result = vm.execute_bytecode_saving_stack(&code, &Rc::new(vec![]));
        assert!(
            result.bits.intersects(crate::value::fiber::SIG_YIELD),
            "the body parks at the yield"
        );
        assert_eq!(
            unsafe { &*heap_ptr }.generation_raw(child_rid.get()),
            gen_before,
            "the adopted member must stay live while the activation is parked \
             (an Owned member is freed only by the node's completion release)",
        );

        let frames = vm.fiber.suspended.take().expect("the yield parked a frame");
        let bits = vm.resume_suspended(frames, crate::value::Value::NIL);
        assert!(bits.is_empty(), "the resumed body completes normally");
        let gen_after = unsafe { &*heap_ptr }.generation_raw(child_rid.get());
        assert!(
            gen_after > gen_before,
            "the adopted member's pages must be returned (generation bumped) by \
             the owner node's subtree drop at the RESUMED activation's normal \
             completion — the node must survive the park \
             (gen {gen_before} -> {gen_after})",
        );
    }

    let after = unsafe { &*heap_ptr }.active_region_count();
    assert!(
        after <= baseline,
        "node + member must be reclaimed at each parked-and-resumed activation's \
         completion — live region count must not grow (baseline={baseline}, after \
         50 activations={after})",
    );
}

/// The node survives REPEATED parks: yield → resume → yield again → resume →
/// complete. The first park carries the node out of the unwinding activation;
/// the resume restores it into the live slot; the second park (during the
/// RESUMED execution) re-captures it; the final completion frees node + member
/// exactly once. Both halves are load-bearing: dropping the restore or the
/// re-capture strands the Owned member (its generation never bumps), and a
/// clone anywhere instead of a move would free it twice (the debug regionstore
/// asserts detonate mid-loop).
#[test]
fn activation_owner_node_survives_repeated_parks() {
    use crate::compiler::bytecode::{Bytecode, Instruction};
    use std::rc::Rc;

    let mut vm = crate::vm::VM::new();
    let heap_ptr = vm.heap_ptr;
    let baseline = unsafe { &*heap_ptr }.active_region_count();

    for _ in 0..50 {
        let (child, child_rid) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        let gen_before = unsafe { &*heap_ptr }.generation_raw(child_rid.get());

        // Body: adopt, yield, (resume) discard the resume value, yield again,
        // (resume) return the second resume value.
        let mut bc = Bytecode::new();
        let idx = bc.add_constant(child);
        bc.emit(Instruction::LoadConst);
        bc.emit_u16(idx);
        bc.emit(Instruction::AdoptIntoActivation);
        bc.emit(Instruction::Nil);
        bc.emit(Instruction::Emit);
        bc.emit_signal_bits(crate::value::fiber::SIG_YIELD);
        bc.emit(Instruction::Pop);
        bc.emit(Instruction::Nil);
        bc.emit(Instruction::Emit);
        bc.emit_signal_bits(crate::value::fiber::SIG_YIELD);
        bc.emit(Instruction::Return);
        let code = crate::value::Code::new(
            Rc::new(bc.instructions),
            Rc::new(bc.constants),
            Rc::new(crate::error::LocationMap::new()),
            Rc::new(vec![]),
        );

        let result = vm.execute_bytecode_saving_stack(&code, &Rc::new(vec![]));
        assert!(
            result.bits.intersects(crate::value::fiber::SIG_YIELD),
            "the body parks at the first yield"
        );

        let frames = vm.fiber.suspended.take().expect("first park");
        let bits = vm.resume_suspended(frames, crate::value::Value::NIL);
        assert!(
            bits.intersects(crate::value::fiber::SIG_YIELD),
            "the resumed body parks again at the second yield"
        );
        assert_eq!(
            unsafe { &*heap_ptr }.generation_raw(child_rid.get()),
            gen_before,
            "the adopted member must stay live across BOTH parks",
        );

        let frames = vm.fiber.suspended.take().expect("second park");
        let bits = vm.resume_suspended(frames, crate::value::Value::NIL);
        assert!(bits.is_empty(), "the twice-resumed body completes normally");
        let gen_after = unsafe { &*heap_ptr }.generation_raw(child_rid.get());
        assert!(
            gen_after > gen_before,
            "the adopted member must be freed at the twice-parked activation's \
             completion — the node must ride park, restore, and re-park \
             (gen {gen_before} -> {gen_after})",
        );
    }

    let after = unsafe { &*heap_ptr }.active_region_count();
    assert!(
        after <= baseline,
        "node + member must be reclaimed after the repeated parks — live region \
         count must not grow (baseline={baseline}, after 50 activations={after})",
    );
}

/// The node rides `ExecResult::activation_owner_node` when the park is built by
/// the CALLER of the already-unwound activation — the fuel-pause channel
/// (docs/impl/region/owner.md § "Owner nodes"). A fuel pause (unlike a yield)
/// creates no suspended frame inside the dispatch loop: the activation unwinds
/// through `execute_bytecode_saving_stack`, which must move the node into the
/// `ExecResult` beside the region map, and the caller builds the park from that
/// result — exactly what `do_fiber_first_resume` does for a fiber body's pause,
/// mirrored here directly. The body adopts a member, then hits a backward jump
/// (the fuel check site) with zero fuel; refueled and resumed, it completes and
/// the node's release frees the member. A `saving_stack` that dropped the node
/// instead of capturing it strands the Owned member: the generation never bumps.
#[test]
fn activation_owner_node_rides_exec_result_across_fuel_pause() {
    use crate::compiler::bytecode::{Bytecode, Instruction};
    use crate::value::{BytecodeFrame, SuspendedFrame};
    use std::rc::Rc;

    let mut vm = crate::vm::VM::new();
    let heap_ptr = vm.heap_ptr;
    let baseline = unsafe { &*heap_ptr }.active_region_count();

    for _ in 0..50 {
        let (child, child_rid) = alloc_in_fresh_region(unsafe { &mut *heap_ptr }, cons());
        let gen_before = unsafe { &*heap_ptr }.generation_raw(child_rid.get());

        // Body: adopt the member, then jump forward over the landing pad to a
        // BACKWARD jump — the fuel check site — that jumps back to the pad
        // (Nil, Return). With zero fuel the backward jump pauses; refueled, it
        // completes.
        //
        //   0: LoadConst idx          3: AdoptIntoActivation
        //   4: Jump +2  (→ 11)        9: Nil    10: Return
        //  11: Jump -7  (→ 9, backward: fuel-checked)
        let mut bc = Bytecode::new();
        let idx = bc.add_constant(child);
        bc.emit(Instruction::LoadConst);
        bc.emit_u16(idx);
        bc.emit(Instruction::AdoptIntoActivation);
        bc.emit(Instruction::Jump);
        bc.emit_i32(2);
        bc.emit(Instruction::Nil);
        bc.emit(Instruction::Return);
        bc.emit(Instruction::Jump);
        bc.emit_i32(-7);
        let code = crate::value::Code::new(
            Rc::new(bc.instructions),
            Rc::new(bc.constants),
            Rc::new(crate::error::LocationMap::new()),
            Rc::new(vec![]),
        );

        vm.fiber.fuel = Some(0);
        let result = vm.execute_bytecode_saving_stack(&code, &Rc::new(vec![]));
        assert!(
            result.bits.intersects(crate::value::SIG_FUEL),
            "the body pauses at the backward jump with zero fuel"
        );
        assert!(
            vm.fiber.suspended.is_none(),
            "a fuel pause parks no frame of its own — the caller builds it"
        );
        assert_eq!(
            unsafe { &*heap_ptr }.generation_raw(child_rid.get()),
            gen_before,
            "the adopted member must stay live across the fuel pause",
        );

        // Build the park from the returned context, exactly as
        // `do_fiber_first_resume` does for a paused fiber body.
        let frame = BytecodeFrame::suspend(
            result.code,
            result.env,
            result.ip,
            result.stack,
            !result.bits.intersects(crate::value::SIG_FUEL),
            result.activation_region_map,
            result.activation_owner_node,
            result.current_closure,
            vm.heap(),
        );
        vm.fiber.fuel = None;
        let bits = vm.resume_suspended(
            vec![SuspendedFrame::Bytecode(frame)],
            crate::value::Value::NIL,
        );
        assert!(bits.is_empty(), "the refueled body completes normally");
        let gen_after = unsafe { &*heap_ptr }.generation_raw(child_rid.get());
        assert!(
            gen_after > gen_before,
            "the adopted member must be freed at the resumed activation's \
             completion — the node must ride the ExecResult out of the paused \
             activation (gen {gen_before} -> {gen_after})",
        );
    }

    let after = unsafe { &*heap_ptr }.active_region_count();
    assert!(
        after <= baseline,
        "node + member must be reclaimed after the fuel-pause round trip — live \
         region count must not grow (baseline={baseline}, after 50 activations={after})",
    );
}
