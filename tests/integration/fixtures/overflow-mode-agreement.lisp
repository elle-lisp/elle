(elle/epoch 12)
# tests/integration/fixtures/overflow-mode-agreement.lisp
#
# Integer overflow must mean the same thing in every mode (docs/intrinsics.md
# § Integer overflow): ints are 64-bit two's-complement and WRAP.
#
# This used to diverge: stdlib `+` folds with `%add`, which in default mode
# lowers to Instruction::Add (wrapping), while `--checked-intrinsics` made it
# a NativeFn calling arithmetic::add_values, which used checked_add and
# signaled :overflow. Same program, two arithmetic semantics, selected by a
# debugging flag whose purpose is type validation. The type proof that makes
# `%add` eligible cannot exclude overflow, so wrapping is the one semantics
# the unchecked instruction, the JIT, the GPU tiers, and --checked-intrinsics
# can all agree on.
#
# Lives here — NOT under tests/elle/ — because the witness property is
# agreement between two PROCESS-GLOBAL modes, which the `elle test` harness
# cannot vary per file. The pin `arithmetic_overflow_mode_agreement` in
# tests/integration/elle_scripts.rs runs this file under default and
# `--checked-intrinsics` and asserts both the printed values and their
# equality across modes.

(def [ok? v] (protect (+ 9223372036854775807 1)))
(assert ok? "int + wraps rather than signaling")
(assert (= v -9223372036854775808) "i64-max + 1 wraps to i64-min")
(println [ok? v])

(def [ok2? v2] (protect (- -9223372036854775808 1)))
(assert ok2? "int - wraps rather than signaling")
(assert (= v2 9223372036854775807) "i64-min - 1 wraps to i64-max")
(println [ok2? v2])

(def [ok3? v3] (protect (* 9223372036854775807 2)))
(assert ok3? "int * wraps rather than signaling")
(assert (= v3 -2) "i64-max * 2 wraps to -2")
(println [ok3? v3])

# Division/remainder overflow corner: MIN/-1 and MIN%-1 wrap, never panic.
(assert (= (/ -9223372036854775808 -1) -9223372036854775808)
        "i64-min / -1 wraps to i64-min")
(assert (= (rem -9223372036854775808 -1) 0) "i64-min rem -1 is 0")

# Unary negation of i64-min wraps to itself.
(assert (= (- -9223372036854775808) -9223372036854775808)
        "negating i64-min wraps to i64-min")
