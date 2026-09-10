(elle/epoch 12)
# audited: 2026-09-09
# A mutual-recursion cycle written as a run of local `defn`s must reclaim its merged
# arena soundly — every member stays live and re-enterable after the arena's drop site
# has passed.
#
# A `defn` run prebinds the same forward cells a `letrec` does: a sibling reads each
# name before its initializer has run, so each keeps a cell, and the cells and closures
# reference each other around the SCC. The closure-cycle merge collapses them onto one
# arena and drops it at the `Begin` that prebound the cells
# (docs/impl/region/letrec.md § "The binder form does not decide the shape").
#
# The soundness hazard this pins is the drop site. Where the run's members leave the
# scope — in the struct a factory returns, or through a closure built outside it — the
# hold is a FOREIGN capture, which is RC-counted and must keep the arena off zero past
# that single decref. If it did not, the caller would hold a closure whose env lives in
# a freed arena: a generation panic on the plain VM, a SIGSEGV under
# `--trace=guardfree`. Every member here is therefore re-entered after the drop site,
# with heap churn in between so a prematurely freed page is recycled and a stale env
# read is loud.
#
# Covers: a factory driven per iteration (the arena minted and freed hundreds of
# times); re-entry of its members across churn; modules held live simultaneously and
# re-entered after many later mint/free cycles; a run whose members are called in place
# and never leave; a run reached through an outer closure that captures a member; and a
# run over a MUTABLE table the members share, so the interior cross-region edges are
# real. Pinned under the UAF oracle by `region_defn_cycle_uaf`
# (tests/integration/elle_scripts.rs).

## The members are used in value position (stored into the returned struct), which
## disables call-site param joins, so a local diverging guard proves each %lt/%sub
## operand (docs/intrinsics.md § The contract).

# The closure-as-module factory: mutually recursive helpers over the factory's own
# mutable table, handed back in a struct. The struct's hold on each member is the
# foreign capture the arena's survival past its drop site rests on.
(defn mk-module []
  (let [t @{}]
    (defn even? [m]
      (when (%not (%int? m)) (error :m))
      (if (%lt m 1) t (odd? (%sub m 1))))
    (defn odd? [m]
      (when (%not (%int? m)) (error :m))
      (put t m true)
      (if (%lt m 1) t (even? (%sub m 1))))
    (let [s {:even even? :odd odd? :table t}]
      s)))

# The same run with nothing handed out: both members are called in place and the
# factory returns a keyword. The arena's whole life is the one activation, so its drop
# site is reached with no foreign hold at all — the case that must NOT pick up a later
# release point.
(defn run-in-place [n]
  (defn ping [m]
    (when (%not (%int? m)) (error :m))
    (if (%lt m 1) :ping (pong (%sub m 1))))
  (defn pong [m]
    (when (%not (%int? m)) (error :m))
    (if (%lt m 1) :pong (ping (%sub m 1))))
  (let [a (ping n)
        b (pong n)]
    (assert (or (= a :ping) (= a :pong)) "run-in-place: ping result")
    (assert (or (= b :ping) (= b :pong)) "run-in-place: pong result")
    :ok))

# A member reached through an OUTER closure that captures it. The outer closure is not
# in the SCC — nothing in the cycle captures it back — so its hold on the member is a
# counted edge into the arena, exactly as the struct's is, and the arena must outlive
# the outer closure rather than its own binding scope.
(defn mk-wrapped []
  (defn walk [m]
    (when (%not (%int? m)) (error :m))
    (if (%lt m 1) :walk (step (%sub m 1))))
  (defn step [m]
    (when (%not (%int? m)) (error :m))
    (if (%lt m 1) :step (walk (%sub m 1))))
  (let [w (fn [k] (walk k))]
    w))

# Build a module, churn fresh heap, then re-enter its members. The churn between the
# arena's drop site and the re-entry is what makes a premature free loud: the page is
# recycled under the closure's env.
(def @churn @[])
(def @i 0)
(while (%lt i 80)
  (let [m (mk-module)]
    (assert (fn? (get m :even)) (string "mk-module: :even at i=" i))
    (push churn (string "mod-" i))
    (let [r ((get m :even) 3)]
      (assert (struct? r)
              (string "mk-module: re-entering :even must reach the shared table "
                      "at i=" i " (arena freed early?)")))
    (let [r2 ((get m :odd) 4)]
      (assert (struct? r2)
              (string "mk-module: re-entering :odd must reach the shared table "
                      "at i=" i)))
    # The table the members share is the interior cross-region edge: reading it back
    # through the struct proves the arena's contents survived the drop site.
    (assert (struct? (get m :table)) (string "mk-module: :table at i=" i)))
  (assign i (%add i 1)))
(assert (= (length churn) 80) "mk-module: churn count")

# Nothing handed out: the arena lives and dies inside one activation.
(def @j 0)
(while (%lt j 80)
  (assert (= (run-in-place j) :ok) (string "run-in-place: at i=" j))
  (push churn (string "inplace-" j))
  (assign j (%add j 1)))

# A member reached only through an outer closure.
(def @k 0)
(while (%lt k 80)
  (let [w (mk-wrapped)]
    (push churn (string "wrapped-" k))
    (let [r (w 3)]
      (assert (or (= r :walk) (= r :step))
              (string "mk-wrapped: the captured member must still run at i=" k))))
  (assign k (%add k 1)))

# Hold several modules live SIMULTANEOUSLY, then re-enter each. Each construction mints
# its own arena, so this pins that one module's release never touches another's — and
# that a module held across many later mint/free cycles is still sound.
(def @held @[])
(def @p 0)
(while (%lt p 40)
  (push held (mk-module))
  (assign p (%add p 1)))
(def @q 0)
(while (%lt q 40)
  (let [m (get held q)]
    (assert (fn? (get m :even))
            (string "held: module " q " must survive later arenas"))
    (assert (struct? ((get m :even) 2))
            (string "held: module " q " must still be re-enterable")))
  (assign q (%add q 1)))

(println "region-defn-cycle-uaf: ok")
