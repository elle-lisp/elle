(elle/epoch 15)
# audited: 2026-11-11
# A compiled tail callee that raises ends the trampoline's activation once, whether its frame parks or not.
# docs/impl/region/unwind.md
#
# Each relay reaches its callee by a tail call, so the trampoline that owns the
# relay's activation runs the callee's compiled code in its place.
#
# A callee that may raise parks its own frame at the raise. A restart answers
# the raise inside that frame, and the callee runs on to read its argument; a
# discard drops the frame. Either way what the trampoline's activation owes
# moves into the parked frame, so the restart's completion or the discard
# releases it. `relay-d` owes the release of the closure it tail-calls, and
# runs in the interpreter, since the JIT refuses a function that builds a
# closure. `relay-n` reaches `relay-d` through `nest`, which also runs in the
# interpreter. `fail-rest` raises with a `&` list built from moved arguments,
# whose surplus references the trampoline releases however the callee ends.
#
# A silent callee parks nothing: the only error that leaves one is a kill or a
# broken proof. The kill ends the activation where it stands, and the worker's
# join answers `:killed`. The kill word stays set, so a kill the trampoline lost
# would raise again at the worker's next charge site. Each worker tail-calls
# its relay, so no charge site follows the callee's return, and a trampoline
# that read the callee's nil as a return finishes the worker with a value.
#
# A lost release strands an object per call, which the gauge reads. A release
# run twice faults under jit-tail-dispatch-error-guardfree.lisp.
#
# The sidecar compiles on the VM thread at each function's first call, and
# turns the graft off, so each tail call stays a call.

(defn fail [s]
  (let [r (error {:error :boom})]
    (string s r)))

(defn fail-rest [a & xs]
  (error {:error :boom :at (string a (first xs))}))

(defn relay [s]
  (fail s))

(defn relay-d [s]
  ((fn [] (fail s))))

(defn relay-rest [n]
  (fail-rest (string "a" n) (string "b" n)))

# `nest` runs in the interpreter, and calls `relay-d` in call position, so the
# interpreter runs `relay-d` on its own dispatch loop and that loop's
# trampoline runs the tail callee.
(defn nest [f s &named unused]
  (let [r (f s)]
    r))

(defn relay-n [s]
  (nest relay-d s))

# ── the faces ───────────────────────────────────────────────────────────────
(defn restarted [via n]
  (let [s (string "v" n)
        f (fiber/new (fn [] [(via s)]) |:error|)]
    (assert (= :boom (get (fiber/resume f) :error)) "the callee raises")
    (assert (= (fiber/resume f "!") [(string "v" n "!")])
            "the restart answers the raise inside the compiled callee")
    (assert (= s (string "v" n)) "the argument outlives the restart")))

(defn discarded [via n]
  (let [s (string "v" n)
        [ok? err] (protect (via s))]
    (assert (and (not ok?) (= :boom (get err :error))) "the callee raises")
    (assert (= s (string "v" n)) "the argument outlives the discard")))

(defn rest-raised [n]
  (let [[ok? err] (protect (relay-rest n))]
    (assert (= (get err :at) (string "a" n "b" n))
            "the rest callee raises with its list read")))

# ── drive ───────────────────────────────────────────────────────────────────
(def window 200)

(defn measure [thunk]
  (def @i 0)
  (while (%lt i 30)
    (thunk i)
    (assign i (%add i 1)))
  (jit/rejections)
  (def before (arena/count))
  (def @j 0)
  (while (%lt j window)
    (thunk j)
    (assign j (%add j 1)))
  (%sub (arena/count) before))

(defn bounded? [d label]
  (assert (%lt d 60) (string label " leaks, delta=" d)))

(bounded? (measure (fn [n] (restarted relay n))) "a compiled relay, restarted")
(bounded? (measure (fn [n] (restarted relay-d n)))
          "an interpreted relay that owes, restarted")
(bounded? (measure (fn [n] (restarted relay-n n)))
          "a nested relay that owes, restarted")
(bounded? (measure (fn [n] (discarded relay n))) "a compiled relay, discarded")
(bounded? (measure (fn [n] (discarded relay-d n)))
          "an interpreted relay that owes, discarded")
(bounded? (measure (fn [n] (discarded relay-n n)))
          "a nested relay that owes, discarded")
(bounded? (measure rest-raised) "a rest callee that raises")

# ── a kill inside a silent tail callee ──────────────────────────────────────
(defn spin [n]
  (numeric!)
  (def @i 0)
  (while (%lt i n) (assign i (%add i 1)))
  i)

(defn relay-k [n]
  (spin n))

(defn relay-ki [n &named unused]
  (spin n))

(defn killed [via compiled?]
  "Run `via` toward no end in a worker, kill the worker once it is about to
  enter, and answer the join's error. The worker refuses to enter unless
  `spin` compiled, and `via` compiled exactly when `compiled?` says. The
  worker tail-calls `via`, so the worker's root trampoline runs the callee,
  and no charge site follows its return."
  (let [[tx rx] (chan)
        h (sys/spawn (fn []
                       (via 3)
                       (jit/rejections)
                       (when (and (vm/config :jit)
                                  (not (and (jit? spin) (= compiled? (jit? via)))))
                         (error {:error :precondition-failed}))
                       (chan/send tx :ready)
                       (via 9223372036854775807)))]
    (chan/select @[rx])
    (sys/kill h)
    (let [[ok? err] (protect (sys/join h))]
      (assert (not ok?) "a killed worker answers no value")
      (get err :error))))

(assert (= :killed (killed spin true)) "a kill ends a compiled tail callee")
(assert (= :killed (killed relay-k true))
        "a kill ends a compiled relay's callee")
(assert (= :killed (killed relay-ki false))
        "a kill ends an interpreted relay's callee")

(when (vm/config :jit)
  (assert (and (jit? fail) (jit? fail-rest) (jit? relay) (jit? relay-rest))
          "precondition: the callees and the compiled relays compile")
  (assert (not (or (jit? relay-d) (jit? relay-ki) (jit? nest)))
          "precondition: relay-d, relay-ki and nest stay interpreted"))

(println "jit-tail-dispatch-error: ok")
