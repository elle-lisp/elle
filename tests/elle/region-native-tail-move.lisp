(elle/epoch 12)
# Counterfactual: a value moved into a NATIVE tail call leaks.
#
# The native-tail corner of the owned-params law. A native primitive borrows its
# args and never reaches a callee `Return`, so a value whose last use is a native
# tail-call argument has no callee to release it (unlike a closure tail callee,
# which releases its owned param). The caller's own value-based release is dead
# past the `TailCall`. So the release must happen in the runtime: after
# `dispatch_native_call` runs (its pass-through retain first), the calling
# convention releases each owned arg. Until then the moved value leaks.
# docs/impl/region-rules.md Rule 8.
#
# `length` is a native that borrows its arg and returns an immediate (int), so
# the only thing that can leak is the MOVED value's region — never a result.
# RED now (the built value / rest list leaks per call); GREEN once the native
# branch of `tail_call_inner` (and the JIT `elle_jit_tail_call` native branch)
# releases owned args after the native runs.

# ── subjects ──────────────────────────────────────────────────────

# (a) a cons built in the body, moved into a native tail call. `(%pair 1 nil)`
# is a 1-element list; `length` returns 1 (immediate). The cons's region's
# value-based release lands past the `TailCall` to the native.
(defn mb-native (n)
  (length (%pair 1 nil)))

# (b) the rest-arg list, built by the calling convention, moved into a native
# tail call.
(defn rest-native (& xs)
  (length xs))

# control: a native tail call on an immediate — nothing heap is moved.
(defn imm-native (n)
  (length nil))

# ── measurement ───────────────────────────────────────────────────
# DIRECT calls (a thunk-wrapped call would itself become a tail call and reroute
# the subject). region AND
# object counts: the rest case commingles (region-count blind).

(defn mb-reg [n]
  (def before (arena/region-count))
  (var i 0)
  (while (%lt i n)
    (mb-native 0)
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

(defn mb-obj [n]
  (def before (arena/count))
  (var i 0)
  (while (%lt i n)
    (mb-native 0)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(defn rest-obj [n]
  (def before (arena/count))
  (var i 0)
  (while (%lt i n)
    (rest-native 1 2 3)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(defn imm-reg [n]
  (def before (arena/region-count))
  (var i 0)
  (while (%lt i n)
    (imm-native 0)
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

(defn imm-obj [n]
  (def before (arena/count))
  (var i 0)
  (while (%lt i n)
    (imm-native 0)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(def i-reg (imm-reg 2000))
(def i-obj (imm-obj 2000))
(def m-reg (mb-reg 2000))
(def m-obj (mb-obj 2000))
(def r-obj (rest-obj 2000))

(println "region-native-tail-move over 2000 iters (region/object deltas):")
(println "  imm-native (control) reg=" i-reg " obj=" i-obj)
(println "  mb-native            reg=" m-reg " obj=" m-obj)
(println "  rest-native                  obj=" r-obj)

# Control: a native tail call on an immediate leaks nothing. Bounded NOW.
(assert (%lt i-reg 50)
        (concat "control: immediate native tail call leaks regions, delta="
                (number->string i-reg)))
(assert (%lt i-obj 50)
        (concat "control: immediate native tail call leaks objects, delta="
                (number->string i-obj)))

# Witness (a): a built value moved into a native tail call leaks region + object.
(assert (%lt m-reg 50)
        (concat "built value moved into a native tail call leaks its region, "
                "delta=" (number->string m-reg)))
(assert (%lt m-obj 50)
        (concat "built value moved into a native tail call leaks its object, "
                "delta=" (number->string m-obj)))

# Witness (b): the rest-arg list moved into a native tail call.
(assert (%lt r-obj 50)
        (concat "rest list moved into a native tail call leaks its conses, "
                "delta=" (number->string r-obj)))

(println "region-native-tail-move: ok")
