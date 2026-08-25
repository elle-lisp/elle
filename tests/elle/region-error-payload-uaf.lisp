(elle/epoch 12)
# Soundness complement of region-error-payload.lisp
# (docs/impl/region/mechanism.md § "An abandoned frame runs the releases it
# still owes" — "What the signal carries is not abandoned — unless the raise
# minted its delivery"). Run under `--trace=guardfree` by the subprocess pin
# `region_error_payload_uaf` in tests/integration/elle_scripts.rs.
#
# An emit-raised error's payload keeps every frame-owed release: the raise
# minted the delivery reference itself, so the payload exemption is withdrawn
# for the abandoned-frame walk and the parked frame's discharge. What must
# survive is every reference the walk does NOT own — the delivery the catcher
# reads, a counted store's, a borrowed payload's owner, a native raise's
# frame-funded delivery, and a restarted frame's replay.
#
# Every read below happens AFTER the raise chain's releases ran, so an
# over-release faults at the deref (guardfree) or trips the generation check.

# ── 1. the catcher reads the emitted payload it was delivered ─────────────────
# The raising frame allocated the payload, so its release is owed and runs.
# The catcher's read rides the delivery reference and must find it whole.

(var i 0)
(while (< i 40)
  (let [e (try
            (error {:error :probe :message (string "m-" i)})
            (catch e e))]
    (assert (= (get e :error) :probe) "the caught payload must be whole")
    (assert (< 0 (length (get e :message)))
            "the payload's own fields must be whole"))
  (assign i (+ i 1)))

# ── 2. the catcher stores the payload into a container that outlives it ───────
# The store funnel counts the sink's reference, so the raise chain's release
# and the fiber's teardown must leave the stored value standing.

(def sink @[])

(assign i 0)
(while (< i 40)
  (try
    (error (string "kept-" i))
    (catch e (push sink e)))
  (assign i (+ i 1)))

(assert (= (length sink) 40) "every caught payload must have reached the sink")
(var n 0)
(while (< n (length sink))
  (assert (= (type-of (get sink n)) :string)
          "a stored payload must outlive its fiber's discharge")
  (assign n (+ n 1)))

# ── 3. a borrowed payload's owner survives the raise ──────────────────────────
# The raise chain owns no reference to a module-level payload, so nothing is
# owed and nothing may be released — the binding must read whole after any
# number of raises.

(def shared {:error :shared :message "borrowed"})

(assign i 0)
(while (< i 40)
  (let [e (try
            (error shared)
            (catch e e))]
    (assert (= (get e :error) :shared) "the borrowed payload must be whole"))
  (assign i (+ i 1)))

(assert (= (get shared :message) "borrowed")
        "the module binding must survive every raise")

# ── 4. a native raise keeps the frame-funded exemption ────────────────────────
# The raising native's payload reaches `fiber.signal` with no mint recorded,
# so the payload exemption stays and the delivery rides the reference the
# frame or the native's own mint left standing. The catcher's read and the
# frame's argument must both be whole.

(defn native-raise [tag]
  (let [arg (string "arg-" tag)
        e (try
            (get arg :not-a-key)
            (catch e e))]
    [(get e :error) (length arg)]))

(assign i 0)
(while (< i 40)
  (let [r (native-raise i)]
    (assert (= (type-of (get r 0)) :keyword) "the native payload must be whole")
    (assert (< 0 (get r 1)) "the raising call's argument must stay whole"))
  (assign i (+ i 1)))

# ── 5. the restarts system replays the parked frame's own releases ────────────
# A resumed `:error` fiber replays past the emit, so the discharge never runs
# and the replay's releases are the frame's own — once each.

(defn restart [tag]
  (let [f (fiber/new (fn []
                       (let [held (string "held-" tag)]
                         (error (string "err-" tag))
                         held)) |:error|)]
    (fiber/resume f)
    (let [first-err (fiber/value f)]
      (fiber/resume f 7)
      [(type-of first-err) (fiber/status f) (type-of (fiber/value f))])))

(assign i 0)
(while (< i 40)
  (let [r (restart i)]
    (assert (= (get r 0) :string) "the first error's payload must be whole")
    (assert (= (get r 1) :dead) "the replayed body must complete")
    (assert (= (get r 2) :string) "the replayed body's own value must be whole"))
  (assign i (+ i 1)))

(println "region-error-payload-uaf: ok")
