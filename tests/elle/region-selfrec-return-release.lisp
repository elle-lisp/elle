(elle/epoch 12)
# A stranded recursive closure the recursion RETURNS still takes the tail-call
# deferred release, and that release must not free it under its caller.
#
# A cell-free self-recursive closure whose letrec/def body is a frame-replacing
# tail call has its region's scope-end `DecrefRegion` emitted as dead code past
# the `TailCall` — or suppressed outright for a `def` — so the runtime deferred
# release is the region's ONLY release channel (docs/impl/selfrec.md). The
# channel is gated on the FIBER frontier alone: a RETURNED closure keeps it,
# because the callee's `Return` mints the caller's reference before
# `trampoline_loop` breaks and runs the deferred decref, leaving the caller's
# reference standing while the deferral drops the frame's own.
#
# What this fixture proves is the second half of that count: the returned
# closure is still LIVE afterwards. Every shape below returns the recursive
# closure and then RE-ENTERS it through the returned handle — the self-call
# re-dispatch reads the executing closure out of its own region, so a region
# freed one reference too early is a stale deref (a generation panic on the
# plain VM, a SIGSEGV under `--trace=guardfree`). The loops interleave string
# and array allocation so a prematurely freed page is RECYCLED before the
# re-entry, turning a silent stale-but-intact read into a loud one.
#
# Pinned under the UAF oracle by `region_selfrec_return_release`
# (tests/integration/elle_scripts.rs); the leak half is pinned by
# `runtime::tests::ownership::recursive_returned_closure_reclaims_per_call` and
# the `recur-local-self-mint` oracle probe.

# ── (1) letrec self-loop that returns itself ─────────────────────────────
(defn make-self [n]
  ## `go` is returned (a value use), which disables call-site param joins, so a
  ## local diverging guard proves the `%lt`/`%sub` operands.
  (defn go [m]
    (when (%not (%int? m)) (error :m))
    (if (%lt m 1) go (go (%sub m 1))))
  (go n))

(let [h (make-self 5)]
  (assert (fn? h)
          "the returned self-recursive closure must survive its own deferred release")
  (assert (fn? (h 3))
          "re-entering the returned closure must not read a recycled region"))

# ── (2) def-route self-loop that returns itself ──────────────────────────
# A self-recursive `def` nested in a lambda has its closure region's would-be-LIVE
# DecrefRegion suppressed rather than stranded as dead code, so it reaches the
# deferral by the other route.
(defn make-def-self [n]
  (def loop-fn
    (fn [m]
      (when (%not (%int? m)) (error :m))
      (if (%lt m 1) loop-fn (loop-fn (%sub m 1)))))
  (loop-fn n))

(let [h (make-def-self 4)]
  (assert (fn? h)
          "the returned self-recursive `def` closure must survive the deferred release")
  (assert (fn? (h 2))
          "re-entering the returned `def` closure must not read a recycled region"))

# ── (3) mutual SCC that returns a member ─────────────────────────────────
# The soundness peer of the two above rather than a third instance of them: the
# cycle MERGES, and its arena rides the member-callee tail deferral — the mutual
# twin of the self-recursive channel these two shapes drive. The merge admits the
# returned member on the same return-mint argument: the member lives in the arena,
# so the callee's `Return` raises the arena's own count before the deferral drops
# the frame's (docs/impl/region/letrec.md § The frontier gate). What must hold here
# is that the handle is still live and re-enterable after the same churn.
# `region-letrec-return-cycle-uaf.lisp` drives that admission's own faces; this is
# its cross-check from the self-recursive side.
(defn make-mutual [n]
  (letrec [ev (fn [m]
                (when (%not (%int? m)) (error :m))
                (if (%lt m 1) ev (od (%sub m 1))))
           od (fn [m]
                (when (%not (%int? m)) (error :m))
                (if (%lt m 1) ev (ev (%sub m 1))))]
    (ev n)))

(let [h (make-mutual 5)]
  (assert (fn? h)
          "the returned SCC member must survive the merged arena's deferred release")
  (assert (fn? (h 4))
          "re-entering the returned member must not read a recycled arena"))

# ── (4) churned loop: build, return, store, re-enter ─────────────────────
# Each iteration returns a fresh recursive closure, parks it in a container that
# outlives the call, allocates over the pages the deferred release just returned,
# and only then re-enters the parked closure.
(def @parked @[])
(def @churn @[])
(var i 0)
(while (< i 60)
  (let [a (make-self 3)
        b (make-def-self 3)
        c (make-mutual 3)]
    (push parked a)
    (push churn (string "churn-" i))
    (push churn @[i i i])
    (assert (fn? (a 1)) "self-loop handle stale after churn")
    (assert (fn? (b 1)) "def-loop handle stale after churn")
    (assert (fn? (c 1)) "mutual-member handle stale after churn"))
  (assign i (+ i 1)))

# The parked handles were built across the whole loop; re-entering the OLDEST one
# last is the longest gap between a deferred release and a use of what it did not free.
(assert (fn? ((get parked 0) 2))
        "the first parked handle must outlive 60 rounds of churn")
(assert (= (length parked) 60) "every round must have parked its handle")

# ── (5) the closure returned through a second frame ──────────────────────
# The handle crosses one more return boundary before it is called, so the
# caller's reference is minted by a `Return` the recursion's own trampoline
# never saw.
(defn forward [n]
  (make-self n))
(let [h (forward 3)]
  (assert (fn? (h 1)) "a handle forwarded through a second frame must stay live"))

(println "region-selfrec-return-release: ok")
