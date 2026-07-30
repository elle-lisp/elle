(elle/epoch 12)
# A `def` evaluates to what it bound, so its initializer is not consumed by the
# binder (docs/impl/region/mechanism.md § "A binder's init release lands after the
# slot store").
#
# Every other binding form's value is its BODY, so an init nothing reads is dead at
# the init itself and the last-use narrowing says so — that is what makes an unused
# `let`'s allocation reclaim promptly (region-unused-let-binding.lisp). A `def` is
# the one binder whose value IS the initializer, so the same narrowing states two
# different things depending on what the `def` sits in:
#
#  - as a STATEMENT the `def`'s own value is discarded, and the demise belongs at
#    the `def` — where the lowerer emits it after the slot store, not before it,
#    which is what makes the release name the stored value instead of the `nil` the
#    binder stamped;
#  - in a CONSUMING position the value flows straight on, and a demise at the
#    initializer would free it under the very expression it was handed to.
#
# This file is the LEAK half — an `arena/count` delta over a fixed window, which
# must be BOUNDED for each subject. The over-free half is
# region-define-init-release-uaf.lisp.

(def window 2000)

(defn measure (thunk warm window)
  (var i 0)
  (while (%lt i warm)
    (thunk)
    (assign i (%add i 1)))
  (def before (arena/count))
  (var j 0)
  (while (%lt j window)
    (thunk)
    (assign j (%add j 1)))
  (%sub (arena/count) before))

# leak: a `def` nothing reads ────────────────────────────────────────────────
# (a) the statement face. The binding is dead the moment it is bound, so the
# region's demise is the `def` itself and its one release has to land where the
# slot already holds the value.
(defn unused-def (n)
  (def x (list n n))
  0)

# (b) the same allocation through the binder that consumes its init — the control
# that says the rate belongs to `def` and not to the allocation.
(defn unused-let (n)
  (let [x (list n n)]
    0))

# (c) two of them: the strand is per region, not per `def`.
(defn unused-def-two (n)
  (def x (list n n))
  (def y (list n n n))
  0)

# (d) an unused `def` inside a loop body, where a demise hoisted out of the body
# would cover N allocations with one release.
(defn unused-def-loop (n)
  (var i 0)
  (while (%lt i 3)
    (def x (list n i))
    (assign i (%add i 1)))
  0)

# (e) the binding IS read, so the demise rides the binding chain to that use — the
# ordinary shape, driven so the file reads the whole rule rather than one corner.
(defn used-def (n)
  (def x (list n n))
  (length x))

(def unused-def-d (measure (fn () (unused-def 3)) 200 window))
(def unused-let-d (measure (fn () (unused-let 3)) 200 window))
(def unused-def-two-d (measure (fn () (unused-def-two 3)) 200 window))
(def unused-def-loop-d (measure (fn () (unused-def-loop 3)) 200 window))
(def used-def-d (measure (fn () (used-def 3)) 200 window))

(println "region-define-init-release deltas over " window " iters:")
(println "  unused: def " unused-def-d "  let " unused-let-d "  two "
         unused-def-two-d "  in-loop " unused-def-loop-d)
(println "  used: def " used-def-d)

# Every leak here is at least one whole object per call, so a surviving strand
# reads >=2000 over the window. 100 is slack for the one-time intercept.
(defn bounded? (d label)
  (assert (%lt d 100) (concat label " leaks, delta=" (number->string d))))

(bounded? unused-let-d "control: an unused `let` init")
(bounded? unused-def-d "the heap init of a `def` nothing reads")
(bounded? unused-def-two-d "two unused `def` inits in one frame")
(bounded? unused-def-loop-d "an unused `def` init allocated per iteration")
(bounded? used-def-d "control: a `def` init the body reads")

(assert (= (used-def 3) 2) "used-def result lost")
(assert (= (unused-def 3) 0) "unused-def result lost")
(assert (= (unused-def-two 3) 0) "unused-def-two result lost")
(assert (= (unused-def-loop 3) 0) "unused-def-loop result lost")

(println "region-define-init-release: ok")
