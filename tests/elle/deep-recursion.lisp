(elle/epoch 12)
# audited: 2026-09-22
# A non-tail recursion 100,000 deep completes, and can park, pause, raise and be
# squelched at the bottom.
#
# Non-tail recursion waits in the fiber, not on the Rust stack
# (docs/impl/vm.md § "Non-tail calls"). None of the recursive calls here is a
# tail call. The runner runs this file with the JIT off and with `--jit=eager`,
# so the compiled tier's hand-off to the interpreter on a low native stack runs
# too.
#
# Counter-factual: while each call nested a Rust frame of 25–30 KB, the VM
# halted every one of these at depth 200 with `:stack-overflow`.

(def depth 100000)

## ── A value flows back up through every level ──────────────────────────
(defn sum-to [n]
  (if (= n 0)
    0
    (+ n (sum-to (- n 1)))))
(assert (= (sum-to depth) 5000050000) "non-tail sum 100,000 deep")

## ── A structure built on the way back up ───────────────────────────────
(defn build [n]
  (if (= n 0)
    ()
    (pair n (build (- n 1)))))
(let [xs (build depth)]
  (assert (= (length xs) depth) "non-tail list build 100,000 deep")
  (assert (= (first xs) depth) "the outermost call's element comes first"))

## ── Mutual recursion through a letrec pair ─────────────────────────────
(defn ping-pong [n]
  (letrec [ping (fn [m]
                  (if (= m 0)
                    0
                    (+ 1 (pong (- m 1)))))
           pong (fn [m]
                  (if (= m 0)
                    0
                    (+ 1 (ping (- m 1)))))]
    (ping n)))
(assert (= (ping-pong depth) depth) "non-tail mutual recursion 100,000 deep")

## ── A spliced call, whose arguments come out of an array ───────────────
(defn splice-sum [n]
  (if (= n 0)
    0
    (+ n (splice-sum ;[(- n 1)]))))
(assert (= (splice-sum depth) 5000050000)
        "non-tail spliced recursion 100,000 deep")

## ── A yield at the bottom parks every paused caller ────────────────────
## The resume value comes back up through all 100,000 callers, so the result
## proves each one was parked and replayed in order.
(defn dive [n]
  (if (= n 0)
    (yield :bottom)
    (+ 1 (dive (- n 1)))))
(let [f (fiber/new (fn [] (dive depth)) |:yield|)]
  (assert (= (fiber/resume f nil) :bottom)
          "a yield 100,000 calls deep reaches the resumer")
  (assert (= (fiber/resume f 0) depth)
          "the resume value returns through every paused caller")
  (assert (= (fiber/status f) :dead) "the deep fiber finishes"))

## ── A fuel pause inside the recursion ──────────────────────────────────
(let [f (fiber/new (fn [] (sum-to depth)) |:fuel|)]
  (fiber/set-fuel f 50000)
  (fiber/resume f nil)
  (assert (= (fiber/status f) :paused) "fuel runs out inside the recursion")
  (fiber/clear-fuel f)
  (fiber/resume f nil)
  (assert (= (fiber/status f) :dead) "the paused recursion finishes")
  (assert (= (fiber/value f) 5000050000)
          "the paused recursion resumes with every caller intact"))

## ── An error at the bottom unwinds every paused caller ─────────────────
(defn fall [n]
  (if (= n 0)
    (error {:error :bottom :message "reached the bottom"})
    (+ 1 (fall (- n 1)))))
(let [[ok? err] (protect ((fn [] (fall depth))))]
  (assert (not ok?) "an error 100,000 calls deep reaches protect")
  (assert (= (get err :error) :bottom) "the error arrives unchanged"))
(assert (= (sum-to depth) 5000050000)
        "recursion runs again after an error unwound 100,000 callers")

## ── A squelch boundary above the recursion ─────────────────────────────
(def squelched-dive (squelch (fn [] (dive depth)) :yield))
(let [[ok? err] (protect ((fn [] (squelched-dive))))]
  (assert (not ok?) "a yield 100,000 calls under a squelch boundary is refused")
  (assert (= (get err :error) :signal-violation)
          "the squelch boundary raises signal-violation"))
(assert (= (sum-to depth) 5000050000)
        "recursion runs again after a squelch discarded 100,000 frames")

## ── The depth cap ──────────────────────────────────────────────────────
(assert (= (vm/config :max-depth) 10000000) "the default depth cap")
(assert (= (get (vm/config) :max-depth) 10000000)
        "the full config struct carries the depth cap")
(vm/config-set :max-depth 200000)
(assert (= (vm/config :max-depth) 200000) "vm/config-set changes the depth cap")
(assert (= (sum-to depth) 5000050000) "a recursion under the cap completes")
(assert (= (get (vm/config-set :max-depth 0) :error) :argument-error)
        "a zero depth cap is refused")
(assert (= (get (vm/config-set :max-depth :deep) :error) :type-error)
        "a depth cap that is not an integer is refused")
(assert (= (vm/config :max-depth) 200000) "a refused cap changes nothing")
(vm/config-set :max-depth 10000000)
