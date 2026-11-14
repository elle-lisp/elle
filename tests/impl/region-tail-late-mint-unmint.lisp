(elle/epoch 15)
# audited: 2026-11-13
# A late mint that no callee receives is given back, on each tier.
# docs/impl/region/handoff.md
#
# A tail call mints an immutable upvalue for a closure callee before it builds
# the callee's environment. A `&named` callee refuses an unknown key while that
# environment is built, so no callee runs to release the mint, and the call
# gives the mint back.
#
# ── The counter-factual ──────────────────────────────────────────────
#
# A mint the call keeps holds the upvalue's region past the closure that
# captured it. Each pass captures a fresh string, so the heap keeps one more
# object per pass. Nothing frees early, so guardfree and `arena/over-frees`
# read such a leak as correct, and only the heap count sees it. The control
# keeps its string on purpose, and must read one object per pass, so a count
# that came back 0 for the wrong reason fails there.
#
# ── The trap ─────────────────────────────────────────────────────────
#
# The callee is a parameter of the closure's maker, so the compiler cannot
# see that the call names an unknown key and refuse it at compile time.

(defn named-x [a &named x]
  a)
(defn mk-refused [k s]
  (fn [] (k s :y 1)))

(def @kept @[])
(defn keep-it [s]
  (push kept s)
  nil)
(defn mk-kept [k s]
  (fn [] (k s)))

(defn refused [tier i]
  (let [f (mk-refused named-x (string "refused-" i))
        [ok? e] (protect (compile/run-on tier f))]
    (assert (not ok?) "the callee refused its arguments")
    (assert (= (get e :error) :argument-error) "the refusal is the unknown key")))

(defn control [tier i]
  (compile/run-on tier (mk-kept keep-it (string "kept-" i))))

# ── The measurement ──────────────────────────────────────────────────
#
# A pass count is the difference between two runs of different lengths, so
# the setup each run pays cancels, and a compile lands in the first run alone.

(def short 32)
(def long 96)

(defn growth [tier pass n]
  (def before (arena/count))
  (var i 0)
  (while (< i n)
    (pass tier i)
    (assign i (inc i)))
  (- (arena/count) before))

(defn per-span [tier pass]
  (growth tier pass 1)
  (- (growth tier pass long) (growth tier pass short)))

(def span (- long short))

# ── The tiers ────────────────────────────────────────────────────────
#
# A build with no JIT compiled in rejects the `:jit` tier, and runs the
# interpreter's rows alone.

(def tiers
  (let [[ok? v] (protect (compile/run-on :jit (fn [] 0)))]
    (if (and (not ok?) (= (get v :error) :tier-rejected))
      [:bytecode]
      [:bytecode :jit])))

(each tier tiers
  (let [g (per-span tier refused)]
    (assert (= g 0)
            (string tier ": a refused call keeps " g " objects over " span
                    " passes")))
  (let [g (per-span tier control)]
    (assert (= g span)
            (string tier ": the control keeps " g " objects over " span
                    " passes, where it keeps one a pass"))))

(println "region-tail-late-mint-unmint: ok")
