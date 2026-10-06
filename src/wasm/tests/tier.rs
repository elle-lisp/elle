// audited: 2026-10-06
// docs/impl/wasm.md
//! A tiered closure's raise reaches the VM on the signal channel, not as its value.
//!
//! The tier admits only a closure inferred silent, and inference is sound, so
//! Elle source cannot build one that raises. This test forges one: a fixture
//! whose recorded signal is silent holds a `MatchFail`, which always raises.
//! That is what a defect in the compiler would hand the tier.
//!
//! The trap: a compiled closure answers on two channels. The status word it
//! returns says whether it parked, and the signal it raised is in
//! `SIGNAL_SLOT`. The counter-factual is a `WasmTier::call` that reads only
//! the status word: the raise reaches the VM as an ordinary return of the
//! error struct, and the caller's `protect` reports success.

use super::*;
use crate::value::{Closure, CodeBuilder, CodePin, SignalBits, SIG_ERROR};

#[test]
fn a_tiered_closures_raise_reaches_the_caller_on_the_signal_channel() {
    crate::value::arena::with_test_region(|| {
        let mut vm = crate::vm::VM::new();
        let forged = LirFixture::new(Arity::Exact(0))
            .closure_id(ClosureId(0))
            .block(
                0,
                &[
                    InstrRef::Const {
                        dst: Reg(0),
                        value: ConstRef::Int(1),
                    },
                    InstrRef::MatchFail {
                        dst: Reg(1),
                        src: Reg(0),
                    },
                ],
                Terminator::Return(Reg(1)),
            )
            .build();
        let template = CodeBuilder::new(Vec::new(), Arity::Exact(0), Vec::new()).build(vm.heap());
        let closure = Closure::new(
            template,
            crate::value::region_slice::RegionSlice::empty(),
            SignalBits::EMPTY,
        );
        let bytecode_ptr = closure.template.bytecode().as_ptr();
        let pin = CodePin::of(vm.heap(), &closure.template);
        let mut tier = crate::wasm::lazy::WasmTier::new().expect("the tier starts");
        assert!(
            tier.compile(pin, &forged.view(), vm.heap_ptr),
            "a closure recorded silent is standalone-emittable"
        );
        let vm_ptr = &mut vm as *mut crate::vm::VM;
        let (_, signal) = tier
            .call(
                vm_ptr,
                bytecode_ptr,
                &std::rc::Rc::new(closure),
                &[],
                crate::value::Value::NIL,
            )
            .expect("the closure runs");
        assert!(
            signal.intersects(SIG_ERROR),
            "the raise must reach the caller as a signal, got {signal}"
        );
    });
}
