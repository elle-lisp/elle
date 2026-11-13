(elle/epoch 15)
# audited: 2026-11-11
# A compiled tail callee's own tail call hands its stranded release to the trampoline's activation, which releases it once.
# docs/impl/region/relocate.md
#
# `relay` builds an adder through `make-adder` and tail-calls it. The adder's
# closure is the relay's to release, and the frame replacement strands that
# release. A compiled relay writes it into the one-shot the next activation
# collects (docs/impl/region/relocate.md).
#
# Each face reaches `relay` by a tail call, so a trampoline runs `relay`
# compiled in its own activation, and runs the adder after it in that same
# activation. No activation opens for the adder, so the trampoline must move
# the one-shot onto its own dues when `relay` answers with the sentinel. The
# trampoline's activation then releases the adder once the adder returns.
#
# A one-shot that nothing moves strands one adder per call, which the gauge
# reads. A one-shot left for the next activation to open is collected by
# `pass`, which the adder calls in call position and which runs in the
# interpreter. That activation ends before the adder reads its captured `k`,
# so it frees the adder under it, and
# region-jit-tail-dispatch-deferral-guardfree.lisp faults at the read.
#
# `outer` is compiled, so the trampoline that runs `relay` is the one a compiled
# caller hands its sentinel to. `outer-i` takes a `&named` parameter, which the
# JIT refuses, so it runs in the interpreter, and the interpreter's trampoline
# runs `relay`. The control calls `relay` in call position, where the
# activation that runs the adder opens after the one-shot is written.
#
# The sidecar compiles on the VM thread at each function's first call, and
# turns the graft off, so each tail call stays a call.

(defn pass [x &named unused]
  x)

# The JIT refuses a function that builds a closure, so `make-adder` runs in
# the interpreter. The adders it builds share one compiled body.
(defn make-adder [k]
  (fn [x]
    (let [y (pass x)]
      (string y k))))

(defn relay [n]
  (let [g (make-adder n)]
    (g "v")))

(defn outer [n]
  (relay n))

(defn outer-i [n &named unused]
  (relay n))

(defn control [n]
  (let [r (relay n)]
    r))

# ── drive ───────────────────────────────────────────────────────────────────
(def window 200)

(defn face [via n]
  (let [k (string "k" n)]
    (assert (= (via k) (string "v" k)) "the adder reads its argument and k")
    (assert (= k (string "k" n)) "k outlives the adder")))

(defn measure [via]
  (def @i 0)
  (while (%lt i 30)
    (face via i)
    (assign i (%add i 1)))
  (jit/rejections)
  (def before (arena/count))
  (def @j 0)
  (while (%lt j window)
    (face via j)
    (assign j (%add j 1)))
  (%sub (arena/count) before))

# A lost release strands the adder, its environment and its capture each call,
# so a leak reads at least `window`.
(defn bounded? [d label]
  (assert (%lt d 60) (string label " leaks, delta=" d)))

(bounded? (measure control) "control: the relay in call position")
(bounded? (measure outer) "a compiled caller's trampoline")
(bounded? (measure outer-i) "the interpreter's trampoline")

(when (vm/config :jit)
  (assert (and (jit? relay) (jit? outer) (jit? (make-adder "k")))
          "precondition: the relay, outer and the adder compile")
  (assert (not (or (jit? make-adder) (jit? pass) (jit? outer-i)))
          "precondition: make-adder, pass and outer-i stay interpreted"))

(println "region-jit-tail-dispatch-deferral: ok")
