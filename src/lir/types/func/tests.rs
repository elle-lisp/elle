use super::*;

/// A single-block function whose body is `instr` followed by `Return(Reg(0))`,
/// with the GPU-friendly defaults (`Arity::Exact`, silent signal, no capture
/// cells) so the only variable under test is the instruction itself.
fn one_instr_func(instr: LirInstr) -> LirFunction {
    let mut func = LirFunction::new(Arity::Exact(1));
    let mut block = BasicBlock::new(Label(0));
    block
        .instructions
        .push(SpannedInstr::new(instr, Span::synthetic()));
    block.terminator = SpannedTerminator::new(Terminator::Return(Reg(0)), Span::synthetic());
    func.blocks.push(block);
    func
}

#[test]
fn numeric_body_is_gpu_eligible_control() {
    // Discriminator: a purely numeric body IS GPU-eligible with these defaults,
    // so the LoadSelf rejection below is that op's doing, not a blanket refusal.
    let func = one_instr_func(LirInstr::Const {
        dst: Reg(0),
        value: LirConst::Int(1),
    });
    assert!(
        func.is_gpu_eligible(),
        "a numeric-only function must be GPU-eligible",
    );
}

#[test]
fn load_self_is_not_gpu_eligible() {
    // LoadSelf reads the executing-closure register — VM/JIT execution-context
    // state with no meaning on an unboxed GPU scalar tier — so a function
    // carrying it must be excluded from GPU compilation.
    let func = one_instr_func(LirInstr::LoadSelf { dst: Reg(0) });
    assert!(
        !func.is_gpu_eligible(),
        "a function loading the executing closure is not GPU-eligible",
    );
}

#[test]
fn adopt_into_activation_is_not_gpu_eligible() {
    // AdoptIntoActivation reaches the fiber's owner-node stack — VM/JIT
    // execution-context state with no meaning on the GPU tier — so a
    // function carrying it must be excluded from GPU compilation.
    let func = one_instr_func(LirInstr::AdoptIntoActivation { child: Reg(0) });
    assert!(
        !func.is_gpu_eligible(),
        "a function adopting into the activation owner node is not GPU-eligible",
    );
}

// ── The allocation-free-by-construction pins ─────────────────────────
//
// The MLIR/SPIR-V tier's region-reclamation state rests on one invariant
// (docs/impl/region/diagnostics.md § "The backend-tier gauge"): the
// eligibility whitelist admits no instruction that can put a heap value in a
// register, and with it no region instruction except the two value-targeted
// RC ops — no-ops on unboxed scalars, so admitting them can never unbalance
// a real region. These pins hold both halves of that argument.

fn static_region(id: u32) -> StaticRegion {
    StaticRegion::new(id).expect("nonzero static slot")
}

#[test]
fn gpu_eligibility_refuses_slot_and_forest_region_instructions() {
    // Slot-resolved RC, adoption, group free, and the coalescing oracle all
    // reach the activation region map or the ownership forest — runtime state
    // the scalar tier does not carry.
    let refused: Vec<LirInstr> = vec![
        LirInstr::IncrefRegion {
            region_id: static_region(2),
        },
        LirInstr::DecrefRegion {
            region_id: static_region(2),
        },
        LirInstr::DecrefCellRegion { src: Reg(0) },
        LirInstr::AdoptRegion {
            parent: Reg(0),
            child: Reg(0),
        },
        LirInstr::AdoptCellRegion {
            parent: Reg(0),
            child: Reg(0),
        },
        LirInstr::FreeRegionGroup {
            members: vec![Reg(0)],
        },
        LirInstr::AssertRegionMatches {
            region_id: static_region(2),
            src: Reg(0),
        },
    ];
    for instr in refused {
        let label = format!("{:?}", instr);
        assert!(
            !one_instr_func(instr).is_gpu_eligible(),
            "{label} reaches region-runtime state and must not be GPU-eligible",
        );
    }
}

#[test]
fn gpu_eligibility_admits_value_targeted_region_rc() {
    // The two value-targeted RC ops are admitted: every instruction that
    // could put a heap value in a register is refused by the whitelist, so
    // on this tier they only ever see unboxed scalars (no region) and skip.
    for instr in [
        LirInstr::IncrefValueRegion { src: Reg(0) },
        LirInstr::DecrefValueRegion { src: Reg(0) },
    ] {
        let label = format!("{:?}", instr);
        assert!(
            one_instr_func(instr).is_gpu_eligible(),
            "{label} is a scalar no-op and must stay GPU-eligible",
        );
    }
}

#[test]
fn gpu_eligibility_refuses_heap_allocation() {
    // The other half of the argument: no allocating instruction is admitted,
    // so no region-managed value is ever minted on the tier — its heap stays
    // with the VM, which reclaims as usual.
    let func = one_instr_func(LirInstr::List {
        dst: Reg(0),
        head: Reg(0),
        tail: Reg(0),
        region: static_region(2),
    });
    assert!(
        !func.is_gpu_eligible(),
        "an allocating instruction must not be GPU-eligible",
    );
}
