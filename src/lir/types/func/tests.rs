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
