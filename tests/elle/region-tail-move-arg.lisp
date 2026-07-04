(elle/epoch 12)
# Counterfactual: a value whose last use is a TAIL-CALL ARGUMENT leaks (the
# owned-params calling convention — docs/impl/region-rules.md Rule 5).
#
# The law (verified): a heap value BOUND IN A SCOPE leaks iff that scope's last
# expression is a tail call AND the value is passed as an argument to it. The
# caller's value-based release for the arg is lowered AFTER the frame-replacing
# `TailCall` (src/lir/lower) → unreachable → the arg's region is never freed. It
# cannot move before the `TailCall`: the tail callee borrows the arg for its
# whole run, so freeing first is a use-after-free. The only leak-free AND
# UAF-free resolution is ownership MOVE: the caller pure-moves the arg into the
# tail callee (no incref, no caller release), and the callee releases it at the
# param's last use (the owned-params calling convention). docs/impl/region-rules.md Rule 8.
#
# This file pins the CLOSURE-tail case for three value kinds the calling
# convention can produce — a value built in the body, the rest-arg list, and the
# `&keys` struct — each moved into a user-closure tail call. RED now (the moved
# value leaks: built value + its region; rest/struct objects); GREEN once
# per-value env regions + owned-params land. Native-tail callees are pinned in
# region-native-tail-move.lisp.

# ── subjects ──────────────────────────────────────────────────────
# `sink` is a closure that ignores its arg and returns an immediate, so the only
# thing that could leak is the MOVED arg's own region — never sink's result.
(defn sink (v)
  0)

# (a) a value built in the body, tail-moved into a closure call. This is the
# concat/fold root in minimal form: `(%pair 1 2)` is bound to the tail-call arg
# position; its region's value-based release lands past the `TailCall`.
(defn move-built (n)
  (sink (%pair 1 2)))

# (b) the rest-arg list, built by the calling convention, tail-moved.
(defn move-rest (& xs)
  (sink xs))

# (c) the `&keys` struct, built by the calling convention, tail-moved.
(defn move-keys (&keys opts)
  (sink opts))

# control: a tail call whose arg is an immediate — nothing heap is moved, so
# nothing can leak. Bounded NOW; guards the measurement harness.
(defn move-imm (n)
  (sink n))

# ── measurement ───────────────────────────────────────────────────
# DIRECT calls in the loop body (a thunk-wrapped call would itself become a tail
# call and reroute the subject). region-count AND object-count:
# the rest/struct cases commingle (region-count blind), only objects witness them.

(defn built-reg [n]
  (def before (arena/region-count))
  (var i 0)
  (while (%lt i n)
    (move-built 0)
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

(defn built-obj [n]
  (def before (arena/count))
  (var i 0)
  (while (%lt i n)
    (move-built 0)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(defn rest-obj [n]
  (def before (arena/count))
  (var i 0)
  (while (%lt i n)
    (move-rest 1 2 3)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(defn keys-obj [n]
  (def before (arena/count))
  (var i 0)
  (while (%lt i n)
    (move-keys :a 1 :b 2)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(defn imm-reg [n]
  (def before (arena/region-count))
  (var i 0)
  (while (%lt i n)
    (move-imm 5)
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

(defn imm-obj [n]
  (def before (arena/count))
  (var i 0)
  (while (%lt i n)
    (move-imm 5)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(def i-reg (imm-reg 2000))
(def i-obj (imm-obj 2000))
(def b-reg (built-reg 2000))
(def b-obj (built-obj 2000))
(def r-obj (rest-obj 2000))
(def k-obj (keys-obj 2000))

(println "region-tail-move-arg over 2000 iters (region/object deltas):")
(println "  move-imm (control) reg=" i-reg " obj=" i-obj)
(println "  move-built         reg=" b-reg " obj=" b-obj)
(println "  move-rest                  obj=" r-obj)
(println "  move-keys                  obj=" k-obj)

# Control: an immediate tail-move leaks nothing. Bounded NOW.
(assert (%lt i-reg 50)
        (concat "control: immediate tail-move leaks regions, delta="
                (number->string i-reg)))
(assert (%lt i-obj 50)
        (concat "control: immediate tail-move leaks objects, delta="
                (number->string i-obj)))

# Witness (a): a value built and moved into a closure tail call — leaks its
# region + object per call.
(assert (%lt b-reg 50)
        (concat "built value moved into a closure tail call leaks its region, "
                "delta=" (number->string b-reg)))
(assert (%lt b-obj 50)
        (concat "built value moved into a closure tail call leaks its object, "
                "delta=" (number->string b-obj)))

# Witness (b): the rest-arg list moved into a closure tail call.
(assert (%lt r-obj 50)
        (concat "rest list moved into a closure tail call leaks its conses, "
                "delta=" (number->string r-obj)))

# Witness (c): the &keys struct moved into a closure tail call.
(assert (%lt k-obj 50)
        (concat "&keys struct moved into a closure tail call leaks, delta="
                (number->string k-obj)))

(println "region-tail-move-arg: ok")
