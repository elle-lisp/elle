(elle/epoch 12)
# Runtime `(eval form)` of a form containing HEAP CONSTANTS (string literals,
# lambdas, quoted data) must compile that form with its constants as ORDINARY
# reclaimable allocations: each is born in its own region, dies at its
# decref_point, and is reclaimed when the eval's transient compilation unwinds.
# So repeated runtime eval stays BOUNDED in object count. docs/impl/region-rules.md
# Rule 8; docs/impl/region-model.md § "Constants lower as ordinary allocations".
#
# Why eval is the sharp case: runtime eval makes compile-time happen at runtime,
# where the compiled form is transient — its constants must die with the eval,
# not outlive it. A string/quoted literal lowered to `MaterializeConst` and a
# region-allocated closure template both satisfy this; a constant held for the
# code object's lifetime would not, and repeated eval would grow without bound.
#
# This is a LEAK witness: an `(arena/count)` delta, not a crash. (A separate UAF
# in the eval-of-quoted-DATA path is pinned in region-eval-quoted-data-leak.lisp.)

# ── attribution ───────────────────────────────────────────────────
# An eval'd string literal is an ordinary heap allocation, so `region-of` gives a
# real reclaimable region (id >= 2), not an immediate (id 0).
(assert (%ge (arena/region-of (eval "x")) 2)
        "eval'd string literal must be an ordinary allocation in a real reclaimable region")

# ── subjects + control ────────────────────────────────────────────
# Warm first (one-time compile/intern effects), then measure the per-iteration
# growth over a fixed window. A bounded eval leaves the count flat; a leaking one
# grows ~1 object/iter.

(defn eval-str (n)
  (var i 0)
  (while (%lt i n)
    (eval "hello")
    (assign i (%add i 1))))

(defn eval-fn (n)
  (var i 0)
  (while (%lt i n)
    (eval '(fn (x) x))
    (assign i (%add i 1))))

# control: a constant-free form has no heap constant to allocate, so it is
# bounded regardless of iteration count.
(defn eval-imm (n)
  (var i 0)
  (while (%lt i n)
    (eval 7)
    (assign i (%add i 1))))

(eval-imm 200)
(def c-before (arena/count))
(eval-imm 2000)
(def c-delta (%sub (arena/count) c-before))

(eval-str 200)
(def s-before (arena/count))
(eval-str 2000)
(def s-delta (%sub (arena/count) s-before))

(eval-fn 200)
(def f-before (arena/count))
(eval-fn 2000)
(def f-delta (%sub (arena/count) f-before))

(println "region-eval-leak over 2000 iters (object deltas):")
(println "  eval 7      (control) obj=" c-delta)
(println "  eval \"hello\"          obj=" s-delta)
(println "  eval (fn (x) x)       obj=" f-delta)

# Control: a constant-free eval allocates no heap constant. Bounded.
(assert (%lt c-delta 50)
        (concat "control: constant-free eval leaks objects, delta="
                (number->string c-delta)))

# Witness (a) — STRING literals lower to `MaterializeConst`, an ordinary
# reclaimable allocation, so repeated eval stays bounded.
(assert (%lt s-delta 50)
        (concat "eval of a string literal leaks objects, delta="
                (number->string s-delta)))

# Witness (b) — closure TEMPLATES are region-allocated, so a lambda's template is
# reclaimed per eval and repeated eval stays bounded.
(assert (%lt f-delta 50)
        (concat "eval of a lambda leaks a closure template, delta="
                (number->string f-delta)))

(println "region-eval-leak: ok")
