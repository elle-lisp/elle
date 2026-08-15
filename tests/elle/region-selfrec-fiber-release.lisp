(elle/epoch 12)
# A stranded recursive closure handed across the FIBER frontier still takes the
# tail-call deferred release, and that release must not free it under the
# resumer or the receiver.
#
# A cell-free self-recursive closure whose letrec/def body is a frame-replacing
# tail call has its region's scope-end `DecrefRegion` emitted as dead code past
# the `TailCall`, so the runtime deferred release is the region's ONLY release
# channel (docs/impl/selfrec.md). The channel is unconditional: a closure that
# crosses the fiber frontier keeps it, because the crossing counts a reference
# of its own — the emit's park retain into `fiber.signal`, which the resumer's
# result release consumes, and `chan/send`'s send-site incref, which holds the
# message until a receive builds the result carrying it. The deferral drops the
# frame's own reference and no other.
#
# What this fixture proves is the second half of that count: the delivered
# closure is still LIVE afterwards. Every shape below hands the closure across a
# fiber boundary, lets the defining activation run its recursion to completion
# (which is where the deferred decref fires), allocates over the pages that
# release could have returned, and only THEN re-enters the delivered handle.
# The self-call re-dispatch reads the executing closure out of its own region,
# so a region freed one reference too early is a stale deref (a generation panic
# on the plain VM, a SIGSEGV under `--trace=guardfree`).
#
# Pinned under the UAF oracle by `region_selfrec_fiber_release`
# (tests/integration/elle_scripts.rs); the leak half is pinned by
# `runtime::tests::ownership::fiber_crossing_recursive_closure_reclaims_per_call`
# and the `recur-local-self-yield` / `recur-local-self-send` oracle probes.

# ── (1) the emit seed: a yielded self-recursive closure ──────────────────
# `go` is yielded (a value use, so a local diverging guard proves the operands),
# then tail-called: the yield parks the activation, the resume runs the
# recursion, and the trampoline's clean break fires the deferred decref.
(defn yield-self [n]
  (letrec [go (fn [m]
                (when (%not (%int? m)) (error :m))
                (if (%lt m 1) go (go (%sub m 1))))]
    (yield go)
    (go n)))

(def @churn @[])
(defn churn-pages [tag]
  (push churn (string "churn-" tag))
  (push churn @[tag tag tag]))

(let [f (fiber/new (fn [] (forever (yield-self 3))) |:yield|)
      h (fiber/resume f nil)]
  (assert (fn? h) "the yielded self-recursive closure must reach the resumer")
  # The recursion — and with it the deferred release — runs on THIS resume.
  (fiber/resume f nil)
  (churn-pages 1)
  (assert (fn? (h 2))
          "re-entering the yielded closure after its deferred release must not read a \
           recycled region"))

# ── (2) the def binder face of the same crossing ─────────────────────────
# A self-recursive `def` nested in a lambda reaches the deferral by the other
# route (its would-be-live `DecrefRegion` is suppressed rather than stranded as
# dead code), so the crossing must be sound through both binders.
(defn yield-def-self [n]
  (def loop-fn
    (fn [m]
      (when (%not (%int? m)) (error :m))
      (if (%lt m 1) loop-fn (loop-fn (%sub m 1)))))
  (yield loop-fn)
  (loop-fn n))

(let [f (fiber/new (fn [] (forever (yield-def-self 4))) |:yield|)
      h (fiber/resume f nil)]
  (assert (fn? h)
          "the yielded self-recursive `def` closure must reach the resumer")
  (fiber/resume f nil)
  (churn-pages 2)
  (assert (fn? (h 2))
          "re-entering the yielded `def` closure must not read a recycled region"))

# ── (3) the Sends seed: a self-recursive closure over a channel ──────────
# `chan/send` is the other fiber-frontier seed, and it needs no fiber at all:
# the send-site incref holds the message in the buffer until the receive builds
# the `[:ok msg]` result carrying it.
(def [snd rcv] (chan))

(defn send-self [n]
  (letrec [go (fn [m]
                (when (%not (%int? m)) (error :m))
                (if (%lt m 1) go (go (%sub m 1))))]
    (chan/send snd go)
    (go n)))

(send-self 3)
(let [h (get (chan/recv rcv) 1)]
  (assert (fn? h) "the sent self-recursive closure must reach the receiver")
  (churn-pages 3)
  (assert (fn? (h 2))
          "re-entering the sent closure after its deferred release must not read a \
           recycled region"))

# ── (4) churned rounds, oldest handle re-entered last ────────────────────
# Each round delivers a fresh recursive closure, parks it in a container that
# outlives the round, and allocates over the pages the deferred release just
# returned. Re-entering the OLDEST handle last is the longest gap between a
# deferred release and a use of what it did not free.
(def @parked @[])
(def gen (fiber/new (fn [] (forever (yield-self 3))) |:yield|))
(var i 0)
(while (< i 40)
  (let [h (fiber/resume gen nil)]
    (fiber/resume gen nil)
    (push parked h)
    (churn-pages i)
    (send-self 2)
    (let [s (get (chan/recv rcv) 1)]
      (assert (fn? (s 1)) "sent handle stale after churn"))
    (assert (fn? (h 1)) "yielded handle stale after churn"))
  (assign i (+ i 1)))

(assert (= (length parked) 40) "every round must have parked its handle")
(assert (fn? ((get parked 0) 2))
        "the first parked handle must outlive 40 rounds of churn")

# ── (5) the handle crosses a second frontier before it is called ─────────
# The delivered closure is sent on to a second consumer, so the reference it is
# called through is neither the frame's nor the first crossing's.
(chan/send snd (get parked 1))
(let [h (get (chan/recv rcv) 1)]
  (assert (fn? (h 2))
          "a handle forwarded across a second frontier must stay live"))

(println "region-selfrec-fiber-release: ok")
