(elle/epoch 12)
# audited: 2026-09-05
# The breaking iteration's own release, run at the break rather than at the
# block's exit label (docs/impl/region/replicate.md).
#
# A region the loop body ALLOCATES is minted once per iteration, so the break
# window refuses to hoist its release to the block's exit — one release there
# would cover whichever iteration's value the slot held last. Every iteration
# that falls through runs its own release and needs nothing. The iteration that
# BREAKS is the last: nothing displaces its value and no later release reaches
# it, so the region strands once per call.
#
# The close is a relocation point the `break` opens at the end of the block it
# leaves, into which a release emitted later is replicated — the same machinery
# a frame-replacing tail call uses, and the same two obligations: escape's
# frame-held admission, and a self-cancelling value route.
#
# This file is the LEAK gauge — an `arena/region-count` delta over a fixed
# window, which must be BOUNDED for each placement of a breaking iteration's
# release, and for the boundary shapes, whose releases must stay where they are.
# The soundness complement is region-break-loop-replica-uaf.lisp.

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
  {:type :data :data 1 :end-stream true})

# subjects ─────────────────────────────────────────────────────────────────────

# (a) the reported shape: a `cond` clause body breaks out of the loop. `msg`'s
# last use is the second clause's TEST, so the branch-arm window anchors its
# release at the `cond`'s own merge — the block the body falls through to before
# jumping back to the loop header — and both breaking bodies jump straight past
# it.
(defn cond-break ()
  (forever
    (let [msg (mk)]
      (cond
        (= msg:type :data) (break nil)
        (= msg:type :error) (error :x)
        true (break nil)))))

# (b) the release sits PAST the branch's merge rather than at it, so the point
# has to outlive the branch that carried the break rather than dying at its
# merge.
(defn merge-after ()
  (var n 0)
  (forever
    (let [msg (mk)]
      (assign n (%add n 1))
      (when (%gt n 1) (break nil))
      (%struct? msg))))

# (c) two live values in the body at the break — the replica is per region, not
# per break. Each is named in a LATER clause test, which is what puts its release
# at the merge rather than inside the arm that runs.
(defn two-values ()
  (forever
    (let [a (mk)
          b (mk)]
      (cond
        (= a:end-stream :never) (break nil)
        (= b:end-stream :never) (break nil)
        (= a:type b:type) (break nil)
        true (break nil)))))

# (d) the break is two branches deep: the `cond` sits inside an `if` arm, so the
# point has to survive the arm boundary the enclosing branch seals.
(defn deep-break (n)
  (forever
    (let [msg (mk)]
      (if (%gt n 0)
        (cond
          (= msg:type :data) (break nil)
          (= msg:type :error) (error :x)
          true (break nil))
        (break nil)))))

# (e) MANY iterations before the break. Every iteration but the last releases at
# its own `decref_point`; only the last needs the replica. A fix that replaced
# the per-iteration release instead of adding to it reads k-1 per call here,
# which is what separates this row from (a).
(defn many-iterations (k)
  (var i 0)
  (forever
    (let [msg (mk)]
      (assign i (%add i 1))
      (cond
        (%lt i k) (%struct? msg)
        true (break i)))))

# (f) the block's value is CONSUMED, so the anchor is the consuming node rather
# than the block itself.
(defn used-result ()
  (+ 0
     (forever
       (let [msg (mk)]
         (cond
           (= msg:end-stream :never) (break 3)
           (= msg:type :data) (break 1)
           true (break 2))))))

# (g) the break CARRIES the loop's own value. Its release is pinned where the
# block's value is consumed — a point the jump reaches — so it is exempt from the
# replica. Freeing it at the break would free what the block is about to hand
# its consumer, which reads here as a crash or a wrong value rather than growth.
(defn carries-value ()
  (forever
    (let [msg (mk)]
      (cond
        (= msg:type :data) (break msg)
        true (break nil)))))

# the point's lifetime ─────────────────────────────────────────────────────────
# The point lives exactly as long as its block is being lowered, and both
# directions of that matter. Here the block sits INSIDE a branch arm, so its exit
# label is reached before the branch's merge: the point must be dead by the time
# the merge emits, or the merge's release is replicated onto a path that already
# ran it. `outer` is live across the whole shape, so an extra release faults.
(defn scope-inner-block (c)
  (let [outer (mk)]
    (if c
      (begin
        (block (let [x (mk)]
                 (when true (break 1))
                 (%struct? x)))
        (length (keys outer)))
      0)))

# The other direction: the break's block ENCLOSES a later branch, whose merge the
# jump skipped, so the point must still be live there.
(defn scope-outer-block (c)
  (block (let [x (mk)]
           (when c (break 1))
           (if (%gt 1 0) 1 2)
           (%struct? x))))

# boundaries ───────────────────────────────────────────────────────────────────
# A lambda nested in the body releases against its own frame's slots, which no
# point of this frame can name. Its releases must stay where they were placed.
(defn bound-lambda (n)
  (forever
    (let [f (fn ()
              (let [x (mk)]
                (%struct? x)))]
      (f)
      (f)
      (when (%gt n -1) (break nil)))))

# controls ─────────────────────────────────────────────────────────────────────
# Each already reads 0: the `if` leaves the release inside an arm rather than at
# a merge the breaks skip, the bare break has nothing between it and the exit,
# the flag never leaves the loop early, and the same `cond` outside a loop has no
# per-iteration allocation to strand.
(defn ctl-if-break ()
  (forever
    (let [msg (mk)]
      (if (= msg:type :data) (break nil) (break nil)))))

(defn ctl-bare-break ()
  (forever
    (let [msg (mk)]
      (break nil))))

(defn ctl-flag ()
  (var done false)
  (while (%not done)
    (let [msg (mk)]
      (cond
        (= msg:type :data) (assign done true)
        (= msg:type :error) (error :x)
        true (assign done true)))))

(defn ctl-noloop ()
  (let [msg (mk)]
    (cond
      (= msg:type :data) nil
      (= msg:type :error) (error :x)
      true nil)))

(def cond-break-d (measure cond-break 200 window))
(def merge-after-d (measure merge-after 200 window))
(def two-values-d (measure two-values 200 window))
(def deep-break-d (measure (fn () (deep-break 1)) 200 window))
(def many-iterations-d (measure (fn () (many-iterations 8)) 200 window))
(def used-result-d (measure used-result 200 window))
(def carries-value-d (measure carries-value 200 window))
(def scope-inner-block-d (measure (fn () (scope-inner-block true)) 200 window))
(def scope-outer-block-d (measure (fn () (scope-outer-block true)) 200 window))
(def bound-lambda-d (measure (fn () (bound-lambda 1)) 200 window))
(def ctl-if-break-d (measure ctl-if-break 200 window))
(def ctl-bare-break-d (measure ctl-bare-break 200 window))
(def ctl-flag-d (measure ctl-flag 200 window))
(def ctl-noloop-d (measure ctl-noloop 200 window))

(println "region-break-loop-replica deltas over " window " iters:")
(println "  cond-break " cond-break-d "  merge-after " merge-after-d
         "  two-values " two-values-d "  deep " deep-break-d)
(println "  many-iterations " many-iterations-d "  used-result " used-result-d
         "  carries-value " carries-value-d)
(println "  scope: inner-block " scope-inner-block-d "  outer-block "
         scope-outer-block-d)
(println "  boundary: lambda " bound-lambda-d)
(println "  controls: if " ctl-if-break-d "  bare " ctl-bare-break-d "  flag "
         ctl-flag-d "  noloop " ctl-noloop-d)

# Every leak in this class is at least one whole region per call, so a surviving
# over-keep reads ~2000 over the window. 100 is slack for the one-time intercept.
(defn bounded? (d label)
  (assert (%lt d 100) (concat label " leaks, delta=" (number->string d))))

(bounded? ctl-if-break-d "control: the `if` arms break")
(bounded? ctl-bare-break-d "control: a bare break in the loop body")
(bounded? ctl-flag-d "control: an assigned flag instead of a break")
(bounded? ctl-noloop-d "control: the same `cond` outside any loop")

(bounded? cond-break-d "a cond clause body breaks out of the loop")
(bounded? merge-after-d "the release sits past the branch's merge")
(bounded? two-values-d "two live values at the break")
(bounded? deep-break-d "the break is two branches deep")
(bounded? many-iterations-d "many iterations, only the last one breaks")
(bounded? used-result-d "the block's value is consumed")
(bounded? carries-value-d "the break carries the loop's own value")

(bounded? scope-inner-block-d
          "the point outlived the block, inside a branch arm")
(bounded? scope-outer-block-d
          "the point died at a nested branch's merge inside its own block")
(bounded? bound-lambda-d "lambda nested in the body: per-activation release")

# Value preservation: adding a release must not change what runs.
(assert (= (used-result) 1) "breaking block result lost")
(def carried (carries-value))
(assert (= carried:type :data)
        "the value the break carried did not survive the replica")
(assert (= carried:data 1)
        "the value the break carried was reclaimed under its consumer")
(assert (= (many-iterations 8) 8) "the loop stopped on the wrong iteration")
(assert (= (deep-break 1) nil) "deep break result lost")
(assert (= (bound-lambda 1) nil) "boundary lambda body diverged")

(println "region-break-loop-replica: ok")
