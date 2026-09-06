// audited: 2026-09-05
// docs/impl/region/owner.md
//! What a compiled activation reclaims when it completes normally.

use super::*;

/// fn(x) -> nil, adopting x's region into the current activation's owner node.
/// The compiled body: load arg 0, `AdoptIntoActivation`, return nil.
fn make_adopt_into_activation_lir() -> LirFunction {
    LirFixture::new(Arity::Exact(1))
        .signal(Signal::silent())
        .block(
            0,
            vec![
                LirInstr::LoadCapture {
                    dst: Reg(0),
                    index: 0,
                },
                LirInstr::AdoptIntoActivation { child: Reg(0) },
                LirInstr::Const {
                    dst: Reg(1),
                    value: crate::lir::LirConst::Nil,
                },
            ],
            Terminator::Return(Reg(1)),
        )
        .build()
}

/// End-to-end exercise of the ACTIVATION OWNER NODE on the JIT
/// (docs/impl/region/owner.md § "Owner nodes — an activation as a forest root"),
/// the compiled twin of
/// `runtime::tests::ownership::activation_owner_node_frees_adopted_member_on_normal_completion`.
/// The compiled body adopts its argument's region into the activation's
/// lazily-minted owner node (`elle_jit_adopt_into_activation`); the compiled
/// `Return` path must free the node (`elle_jit_release_activation_dues`),
/// whose subtree drop reclaims the member — its generation bumps and the live
/// region count stays bounded across 50 calls. The member is Owned (count
/// consumed by the adopt), so if the Return-path release is not emitted, NOTHING
/// reclaims it — node + member entries survive every call.
#[test]
fn adopt_into_activation_frees_member_at_compiled_return() {
    use crate::value::heap::{HeapObject, Pair};

    let lir = make_adopt_into_activation_lir();
    let compiler = JitCompiler::new().expect("Failed to create compiler");
    let code = compiler
        .compile(&lir, None, Vec::new())
        .expect("Failed to compile");

    let mut vm = crate::vm::VM::new();
    let heap_ptr = vm.heap_ptr;
    let baseline = unsafe { &*heap_ptr }.active_region_count();

    for _ in 0..50 {
        let (child, child_rid) = crate::value::arena::alloc_in_fresh_region(
            unsafe { &mut *heap_ptr },
            HeapObject::Pair(Pair::new(
                crate::value::Value::NIL,
                crate::value::Value::NIL,
            )),
        );
        let gen_before = unsafe { &*heap_ptr }.generation_raw(child_rid.get());

        let args = [child];
        let value = unsafe {
            code.call(
                std::ptr::null(),
                args.as_ptr(),
                1,
                &mut vm as *mut crate::vm::VM as *mut (),
                0,
                0,
            )
        }
        .to_value();
        assert!(value.is_nil(), "the adopt-and-return body returns nil");

        let gen_after = unsafe { &*heap_ptr }.generation_raw(child_rid.get());
        assert!(
            gen_after > gen_before,
            "the adopted member's pages must be returned (generation bumped) by \
             the owner node's release on the compiled Return path \
             (gen {gen_before} -> {gen_after})",
        );
    }

    let after = unsafe { &*heap_ptr }.active_region_count();
    assert!(
        after <= baseline,
        "node + member must be reclaimed at each compiled call's completion — live \
         region count must not grow (baseline={baseline}, after 50 calls={after})",
    );
}
