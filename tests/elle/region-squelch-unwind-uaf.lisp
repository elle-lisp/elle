(elle/epoch 12)
# Soundness complement of region-squelch-unwind.lisp
# (docs/impl/region/mechanism.md § "A squelch boundary abandons frames the same
# way, so it runs the same walk"). Run under `--trace=guardfree` by the
# subprocess pin `region_squelch_unwind_uaf` in tests/integration/elle_scripts.rs.
#
# A squelch/attune boundary abandons the frames between the emitting site and
# itself, so the discard runs the releases each of them still owed. Each is a
# release that frame genuinely had, run earlier than it would have been — and
# the fiber SURVIVES, so what has to be whole afterwards is everything the
# surviving side still reads.
#
# Four faces.
#
# 1. The CATCHING activation's own values. It is not abandoned, and it goes on
#    running with the violation in hand.
# 2. An OUTER, non-discarded frame's values. The boundary sits between it and
#    the emitting frame, so the discard walks past neither into it.
# 3. A value the abandoned frame STORED somewhere that outlives it. The store
#    funnel took a counted reference, so the discard's release drops the frame's
#    own and no more.
# 4. REPETITION. A region a discard over-releases drains within a few
#    iterations, and the scheduler machinery behind the boundary is reused
#    across them, so every subject runs in a loop and reads its values after.
#
# Every read below happens AFTER the discard ran, so an over-release faults at
# the deref (guardfree) or trips the generation check.

# ── 1. the catching activation's own values ──────────────────────────────────
# `held` belongs to the frame that CATCHES, and the squelched body holds a
# pending value of its own for the discard to release.

(def one-body
  (squelch (fn []
             (let [s (string "inner-" 1)]
               (begin
                 (emit :yield 1)
                 s))) :yield))

(defn catch-holds [tag]
  (let [held (string "held-" tag)]
    (let [[ok e] (protect (one-body))]
      [(length held) ok (get e :error)])))

(var i 0)
(while (< i 40)
  (let [r (catch-holds i)]
    (assert (< 0 (get r 0)) "the catching frame's own value must be whole")
    (assert (= (get r 1) false) "the boundary must raise")
    (assert (= (get r 2) :signal-violation)
            "the violation must be readable after the discard"))
  (assign i (+ i 1)))

# ── 2. an outer, non-discarded frame's values ────────────────────────────────
# `outer` is bound by a frame ABOVE the boundary. The frames the discard
# abandons are the emitting one and those between it and the boundary, so a
# release reaching `outer` is one the discard was never entitled to run.

(defn outer-across [tag]
  (let [outer (string "outer-" tag)]
    (begin
      (protect (one-body))
      (protect (one-body))
      (length outer))))

(assign i 0)
(while (< i 40)
  (assert (< 0 (outer-across i))
          "an outer frame's value must survive two discards under it")
  (assign i (+ i 1)))

# ── 3. a value the abandoned frame stored into a longer-lived container ──────
# `sink` outlives every call. The squelched body stores a fresh string into it
# and then emits with that string's own binding still live, so the discard
# releases the frame's reference and the sink's counted one must keep the value
# alive.

(def sink @[])

(def store-body
  (squelch (fn []
             (let [s (string "kept" 1)]
               (begin
                 (push sink s)
                 (emit :yield 1)
                 s))) :yield))

(assign i 0)
(while (< i 40)
  (protect (store-body))
  (assign i (+ i 1)))

(assert (= (length sink) 40) "every stored value must have reached the sink")
(var n 0)
(while (< n (length sink))
  (assert (= (type-of (get sink n)) :string)
          "a value the abandoned frame stored must outlive the discard")
  (assign n (+ n 1)))

# ── 4. the slot route, and an attune boundary ────────────────────────────────
# A closure the abandoned frame allocated is released by the slot route, whose
# receipt is the activation map rather than a nil stamp. The enclosing frame
# calls it after the boundary, so an over-release faults on the read.

(def slot-body
  (attune |:error|
          (fn []
            (let [x 5
                  f (fn [] (+ x 1))]
              (begin
                (emit :yield 1)
                (f))))))

(defn slot-across [tag]
  (let [g (fn [] tag)]
    (begin
      (protect (slot-body))
      (g))))

(assign i 0)
(while (< i 40)
  (assert (= (slot-across i) i)
          "the catching frame's own closure must survive the slot route")
  (assign i (+ i 1)))

# ── 5. the scheduler survives the discards ───────────────────────────────────
# Fresh fiber machinery after 160 boundaries: a yield round-trip must read
# intact state, which is where a drained scheduler region would surface.

(def coro (fiber/new (fn [] (yield (string "round" 1))) |:yield|))
(fiber/resume coro nil)
(assert (= (type-of (fiber/value coro)) :string)
        "a fresh fiber must yield intact after the discards")

(println "region-squelch-unwind-uaf: ok")
