(elle/epoch 12)
# `break` transfers its value to the block; it does not consume it
# (docs/impl/region/mechanism.md § "`break` transfers its value; it does not
# consume it").
#
# A `block`'s value is its fall-through value OR the value of any `break`
# targeting it. `break` lowers to a store into the block's result slot plus a
# jump to the block's exit label, so control leaves the body there: every
# release the lowerer placed at a `decref_point` INSIDE the body is jumped over.
# Anchoring the broken value's release inside the body means it never runs — the
# value is held to fiber teardown, one region per break.
#
# So the broken value is anchored where the BLOCK's value is consumed: its
# regions flow out of the block as the block's own result regions, and each is
# pinned to `last_use[block]` — the block node itself when nothing consumes the
# result, which the lowerer emits after the exit label, so the release fires on
# the break path and the fall-through path alike.
#
# This file is the LEAK gauge for that transfer — an `arena/region-count` delta
# over a fixed window, which must be BOUNDED for every placement of the broken
# value: bare, `let`-bound, through a branch, out of a `while`, out of a nested
# block, and with the block's result consumed. The soundness complement — the
# broken value must SURVIVE a later read — is region-break-transfer-uaf.lisp;
# the per-op rates are the `break-value*` probes in tests/elle/oracle.lisp.

(def window 2000)

(defn measure (thunk warm window)
  (var i 0)
  (while (%lt i warm)
    (thunk)
    (assign i (%add i 1)))
  (def before (arena/region-count))
  (var j 0)
  (while (%lt j window)
    (thunk)
    (assign j (%add j 1)))
  (%sub (arena/region-count) before))

(defn mk ()
  {:a 1})
(defn mk2 ()
  {:b 2})

# subjects ─────────────────────────────────────────────────────────────────────

# (a) the value is minted directly in break position, block value discarded.
(defn brk-bare ()
  (block (break (mk)))
  nil)

# (b) the value is `let`-bound first — the binding's own `decref_point` sits
# inside the body, exactly where the break jumps over it.
(defn brk-let ()
  (block (let [x (mk)]
           (break x)))
  nil)

# (c) a heap LITERAL in break position (no call result to route through).
(defn brk-literal ()
  (block (break {:a 1}))
  nil)

# (d) the block's result is CONSUMED — the release must move past the read, not
# just past the block.
(defn brk-used ()
  (let [r (block (let [x (mk)]
                   (break x)))]
    (get r :a)))

# (e) a branch in break position: either arm becomes the block's value, so
# neither may be anchored inside the body.
(defn brk-branch (n)
  (block (break (if (%gt n 0) (mk) (mk2))))
  nil)

# (f) out of a `while` — the implicit `:while` block the analyzer wraps the loop
# in, so the exit label sits outside the loop.
(defn brk-while ()
  (block (while true (break (mk))))
  nil)

# (g) out of a NESTED block to the outer one: the value crosses two exit labels.
(defn brk-nested ()
  (block :outer
    (block :inner
      (break :outer (mk))))
  nil)

# (h) the `forever`/`break` idiom with the block in the FUNCTION's tail
# position, over a binding the loop reassigns: the broken value is the
# function's RESULT and aliases its argument. The `lib/http` SSE-drain shape.
# Its per-iteration scratch belongs to another class (the reassign 1-slot gate,
# gauged by the oracle), so this row is driven for its VALUE — and for guardfree
# soundness in region-break-transfer-uaf.lisp — not as a bounded-delta row.
(defn brk-tail-loop (s)
  (def @rest s)
  (block :drain
    (forever
      (let [nl (string/find rest "\n")]
        (when (nil? nl) (break :drain rest))
        (assign rest (slice rest (inc nl)))))))

# controls ─────────────────────────────────────────────────────────────────────
# The same shapes with no break — bounded already, so a red row above is the
# break path and not the surrounding scaffolding.
(defn ctl-block ()
  (block (let [x (mk)]
           x))
  nil)
(defn ctl-used ()
  (let [r (block (mk))]
    (get r :a)))

(def brk-bare-d (measure (fn () (brk-bare)) 200 window))
(def brk-let-d (measure (fn () (brk-let)) 200 window))
(def brk-literal-d (measure (fn () (brk-literal)) 200 window))
(def brk-used-d (measure (fn () (brk-used)) 200 window))
(def brk-branch-d (measure (fn () (brk-branch 1)) 200 window))
(def brk-while-d (measure (fn () (brk-while)) 200 window))
(def brk-nested-d (measure (fn () (brk-nested)) 200 window))
(def ctl-block-d (measure (fn () (ctl-block)) 200 window))
(def ctl-used-d (measure (fn () (ctl-used)) 200 window))

(println "region-break-transfer deltas over " window " iters:")
(println "  bare " brk-bare-d "  let " brk-let-d "  literal " brk-literal-d
         "  used " brk-used-d)
(println "  branch " brk-branch-d "  while " brk-while-d "  nested "
         brk-nested-d)
(println "  controls: block " ctl-block-d "  used " ctl-used-d)

# Every leak in this class is one whole region per break, so a surviving
# over-keep reads ~2000 over the window. 100 is slack for the one-time intercept.
(defn bounded? (d label)
  (assert (%lt d 100) (concat label " leaks, delta=" (number->string d))))

(bounded? ctl-block-d "control: block with no break")
(bounded? ctl-used-d "control: consumed block with no break")

(bounded? brk-bare-d "break of a fresh call result")
(bounded? brk-let-d "break of a let-bound value")
(bounded? brk-literal-d "break of a heap literal")
(bounded? brk-used-d "break whose block result is consumed")
(bounded? brk-branch-d "break of a branch value")
(bounded? brk-while-d "break out of a while")
(bounded? brk-nested-d "break out of a nested block")

# Value preservation: the transfer must not change what the block evaluates to.
(assert (= (brk-used) 1) "break-carried value lost through the block result")
(assert (= (get (block (break (mk))) :a) 1) "bare break value lost")
(assert (= (get (block (while true (break (mk2)))) :b) 2)
        "break-out-of-while value lost")
(assert (= (get (block :outer
                  (block :inner
                    (break :outer (mk)))) :a) 1) "nested break value lost")
(assert (= (brk-tail-loop "p\nq\nz") "z") "tail-block loop break value lost")

(println "region-break-transfer: ok")
