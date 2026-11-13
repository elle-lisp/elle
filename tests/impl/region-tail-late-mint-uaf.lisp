(elle/epoch 15)
# audited: 2026-11-13
# A borrowed upvalue a tail call hands its callee is minted exactly when the callee will release it.
# docs/impl/region/handoff.md
# docs/impl/region/signalexit.md
#
# The sidecar arms guardfree, so a released page is unmapped and a stale read
# faults at its deref, and compiles on the VM thread, so each loop runs its
# first ten iterations in the interpreter and the rest compiled.
#
# A tail call whose argument is an immutable upvalue mints it at the
# `TailCall`, once the runtime knows the callee: a closure that replaces the
# frame takes the mint and releases it as its parameter, and a native takes
# none. Each shape below reads the upvalue on every pass, so a mint the
# runtime skipped for a closure, or a release it ran for a mint it never made,
# frees the closure's captured string under the next pass.
#
# ── The counter-factual ──────────────────────────────────────────────
#
# A mint skipped for a closure callee frees the upvalue on the first pass, and
# guardfree faults at the next pass's read. A stash the runtime neither minted
# nor stamped `nil` is released by the fall-through or a replay anyway, which
# the `arena/over-frees` reading at the end counts where guardfree has no page
# to fault on.

(def over-frees (arena/over-frees))
(def rounds 40)

# ── 1. a closure callee ──────────────────────────────────────────────────────

(defn measure [g]
  (length g))
(defn mk-forward [s]
  (fn [k] (k s)))

(def forward (mk-forward (string "forwarded")))
(var i 0)
(while (< i rounds)
  (assert (= (forward measure) 9) "the closure callee read the upvalue")
  (assign i (inc i)))

# ── 2. a native that completes ───────────────────────────────────────────────

(defn mk-native [s]
  (fn [] (length s)))
(def native (mk-native (string "native")))
(assign i 0)
(while (< i rounds)
  (assert (= (native) 6) "the native read the upvalue")
  (assign i (inc i)))

# ── 3. a native that raises ──────────────────────────────────────────────────
# The signal exit finds no mint at the stash, and stamps it `nil`.

(defn mk-raise [s]
  (fn [] (+ s 1)))
(def raised (string "raised"))
(def raise (mk-raise raised))
(assign i 0)
(while (< i rounds)
  (let [[ok? _] (protect (raise))]
    (assert (not ok?) "the native raised"))
  (assert (= (length raised) 6) "the upvalue survives the raise")
  (assign i (inc i)))

# ── 4. a native that carries a fiber ─────────────────────────────────────────
# `fiber/resume` in tail position hands the scheduler the captured fiber, which
# runs and ends inside the call.

(defn mk-carry [n]
  (let [inner (fiber/new (fn [] (string "inner-" n)) 1)]
    [inner (fn [] (fiber/resume inner))]))
(assign i 0)
(while (< i rounds)
  (let [[inner carry] (mk-carry i)
        outer (fiber/new carry 1)]
    (assert (= (fiber/resume outer) (string "inner-" i))
            "the carried fiber's value")
    (assert (= (type-of inner) :fiber) "the carried fiber survives its exit"))
  (assign i (inc i)))

# ── 5. a native that suspends with the upvalue as its payload ────────────────
# A dynamic `emit` parks the frame at its fall-through, which releases the
# stash on resume. So the runtime mints the payload at the suspend, and the
# stash keeps that mint for the replay to release.

(def sig :yield)
(defn mk-emit [s]
  (fn [] (emit sig s)))
(assign i 0)
(while (< i rounds)
  (let [s (string "payload-" i)
        body (mk-emit s)
        f (fiber/new body |:yield|)]
    (assert (= (fiber/resume f) s) "the suspended payload")
    (fiber/resume f)
    (assert (= (length s) (length (string "payload-" i)))
            "the payload survives the replay"))
  (let [s (string "abandoned-" i)
        f (fiber/new (mk-emit s) |:yield|)]
    (assert (= (fiber/resume f) s) "the payload of an abandoned park")
    (assert (= (length s) (length (string "abandoned-" i)))
            "the payload survives the abandoned park"))
  (assign i (inc i)))

# ── 6. a self tail call ──────────────────────────────────────────────────────
# Compiled code turns the call into a jump that replaces no frame, so the loop
# mints the upvalue itself before each jump, and each pass releases it as its
# parameter.

(defn mk-loop [s]
  (letrec [go (fn [n acc] (if (= n 0) (length acc) (go (- n 1) s)))]
    go))
(def looping (mk-loop (string "looped")))
(assign i 0)
(while (< i rounds)
  (assert (= (looping 20 "") 6) "every pass read the upvalue")
  (assign i (inc i)))

# ── 7. a native that suspends with the upvalues beside its payload ───────────
# `port/write` parks an `IoRequest` it built, which names the port and the
# bytes without a count. The suspend leaves every stash standing for the
# replay, so the runtime mints the port and the bytes too, and the replay
# releases each. A stash written back without its mint is released past the
# closure's count, so the second write reads a freed port or freed bytes.

(defn mk-write [p data]
  (fn [] (port/write p data)))
(def sink (port/open "/dev/null" :write))
(assign i 0)
(while (< i rounds)
  (let [data (string "written-" i)
        write (mk-write sink data)]
    (write)
    (write)
    (assert (= (length data) (length (string "written-" i)))
            "the bytes survive both replays"))
  (assign i (inc i)))
(port/close sink)

(assert (= (arena/over-frees) over-frees) "no release ran past zero")

(println "region-tail-late-mint-uaf: ok")
