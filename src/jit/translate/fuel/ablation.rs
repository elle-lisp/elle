// audited: 2026-10-22
//! The shapes a silent fuel charge can take, for the fuel bench to compare.
//!
//! benches/AGENTS.md
//! docs/impl/jit.md
//!
//! Only a build with the `ablation` feature compiles this module. The knob is
//! thread-local, because `compile/run-on :jit` compiles on the calling thread,
//! and it starts at the shape every other build emits.

use super::*;
use cranelift_codegen::ir::Value as IrValue;
use std::cell::Cell;

/// One rung of the ladder: what a silent charge site holds. Each rung is one
/// operation more than the one before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChargeShape {
    /// Nothing: the floor.
    None,
    /// A load, a subtract and a store.
    Decrement,
    /// The same, and the compare and extend that stop the word at zero.
    Saturating,
    /// A load, and a branch on zero around a subtract and a store.
    Branch,
    /// The branch, after the kill check: a load of the kill word's pointer,
    /// a load of the word, a compare and a branch to the cold block that
    /// kills. The charge every other build emits.
    Check,
}

impl ChargeShape {
    /// Every rung, from the floor up.
    pub const LADDER: [ChargeShape; 5] = [
        ChargeShape::None,
        ChargeShape::Decrement,
        ChargeShape::Saturating,
        ChargeShape::Branch,
        ChargeShape::Check,
    ];

    /// The rung's name, as the bench reports it.
    pub fn name(self) -> &'static str {
        match self {
            ChargeShape::None => "none",
            ChargeShape::Decrement => "decrement",
            ChargeShape::Saturating => "saturating",
            ChargeShape::Branch => "branch",
            ChargeShape::Check => "check",
        }
    }
}

thread_local! {
    static SHAPE: Cell<ChargeShape> = const { Cell::new(ChargeShape::Check) };
}

/// Choose the shape every silent charge this thread compiles from now on takes.
pub fn set_charge_shape(shape: ChargeShape) {
    SHAPE.with(|s| s.set(shape));
}

/// The shape a silent charge compiled on this thread takes.
pub fn charge_shape() -> ChargeShape {
    SHAPE.with(Cell::get)
}

impl<'a> FunctionTranslator<'a> {
    /// The charge at a silent site, in the shape the knob names. The `check`
    /// rung is the charge every other build emits, so the bench's ladder tops
    /// out at what ships.
    pub(super) fn emit_silent_charge(
        &mut self,
        builder: &mut FunctionBuilder,
        vm: IrValue,
    ) -> Result<(), JitError> {
        let (offset, flags) = Self::budget_word();
        match charge_shape() {
            ChargeShape::None => {}
            ChargeShape::Decrement => {
                let word = builder.ins().load(I64, flags, vm, offset);
                let less_one = builder.ins().iadd_imm_s(word, -1);
                builder.ins().store(flags, less_one, vm, offset);
            }
            ChargeShape::Saturating => {
                let word = builder.ins().load(I64, flags, vm, offset);
                let left = builder.ins().icmp_imm_u(IntCC::NotEqual, word, 0);
                let taken = builder.ins().uextend(I64, left);
                let spent = builder.ins().isub(word, taken);
                builder.ins().store(flags, spent, vm, offset);
            }
            ChargeShape::Branch => {
                let word = builder.ins().load(I64, flags, vm, offset);
                self.emit_spend_unless_zero(builder, vm, word);
            }
            ChargeShape::Check => self.emit_checked_silent_charge(builder, vm)?,
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::eval_all;
    use crate::runtime::RuntimeCore;
    use crate::value::Value;

    /// Two metered fibers with five units each: `quiet` runs a silent function
    /// with no loop, and `loud` runs a silent loop of a thousand trips. The
    /// program answers what each has left, and the loop's result.
    const PROGRAM: &str = "
(defn spin [] (numeric!) (def @i 0) (while (%lt i 1000) (assign i (%add i 1))) i)
(defn still [] (numeric!) 0)
(def quiet (fiber/new (fn [] (compile/run-on :jit still)) |:fuel|))
(fiber/set-fuel quiet 5)
(fiber/resume quiet)
(def loud (fiber/new (fn [] (compile/run-on :jit spin)) |:fuel|))
(fiber/set-fuel loud 5)
(fiber/resume loud)
(list (fiber/fuel quiet) (fiber/fuel loud) (fiber/value loud))";

    /// The program's three answers under `shape`, with the knob put back.
    fn run(shape: ChargeShape) -> Vec<Value> {
        set_charge_shape(shape);
        let mut core = RuntimeCore::bare();
        let (vm, symbols, cctx) = core.parts();
        let out = eval_all(PROGRAM, symbols, vm, cctx, "<ablation>").expect("the program runs");
        let items = out.list_to_vec().expect("a list of three immediates");
        set_charge_shape(ChargeShape::Check);
        items
    }

    #[test]
    fn the_knob_starts_at_the_shape_every_other_build_emits() {
        // The kill check is the silent charge of a production build
        // (benches/AGENTS.md), so a bench that never turns the knob measures
        // what ships.
        assert_eq!(charge_shape(), ChargeShape::Check);
    }

    // The counter-factual: a translator that ignores the knob spends the
    // loud fiber's budget to zero under every shape.
    #[test]
    fn the_floor_charges_nothing_in_a_silent_loop() {
        assert_eq!(
            run(ChargeShape::None),
            [Value::int(4), Value::int(4), Value::int(1000)],
            "the loud fiber keeps what the quiet one keeps: the call cost one unit, the loop none"
        );
    }

    // A plain decrement runs past zero, and a word past the largest budget
    // reads as no budget. That is why the rung is a rung and not a shape.
    #[test]
    fn a_plain_decrement_runs_a_spent_budget_past_the_largest() {
        assert_eq!(
            run(ChargeShape::Decrement),
            [Value::int(4), Value::NIL, Value::int(1000)]
        );
    }

    #[test]
    fn every_stopping_shape_spends_the_budget_and_stops_at_zero() {
        for shape in [
            ChargeShape::Saturating,
            ChargeShape::Branch,
            ChargeShape::Check,
        ] {
            assert_eq!(
                run(shape),
                [Value::int(4), Value::int(0), Value::int(1000)],
                "under {shape:?}"
            );
        }
    }
}
