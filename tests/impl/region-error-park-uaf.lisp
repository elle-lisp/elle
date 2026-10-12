(elle/epoch 15)
# audited: 2026-10-06
# A callee's parked error chain releases nothing a reader still holds, on a restart or on any seam that ends it.
# docs/impl/region/unwind.md
#
# The leak face is region-error-park.lisp. An error parks every frame from the
# raise to the fiber body, and each seam that ends the park runs a release:
# the restart runs the frames' own, and a dropped fiber, a cancel and a
# discarding host run the ones the frames still owe. Each is a chance to free
# a value a reader holds past it: the restart value, a value the caller shares
# with the parked frames, or a payload built from a parked frame's binding.
#
# Each face reads a HEAP value after the seam and compares it with an equal
# value built afterwards. Where the freed page was recycled the compare fails
# on any run; where it was not, the sidecar arms guardfree, which unmaps the
# page so the read faults.

# Read by the parked frames and never released by any of them.
(def shared (string "shared-" 1))

(defn holder [n]
  (let [h (string "h" n)]
    [(get nil :x) h shared]))
(defn outer-holder [n]
  (let [o (string "o" n)]
    [(holder n) o shared]))

# The payload holds the raising frame's own binding.
(defn payload-holder [n]
  (let [h (string "h" n)]
    [(error {:error :held :data h}) h]))
(defn payload-outer [n]
  (let [o (string "o" n)]
    [(payload-holder n) o]))

(defn held? [p n]
  (and (= (get p :error) :held) (= (get p :data) (string "h" n))))

# ── the chain restarted, the restart value read from the result ──────────────
(defn face-restart [n]
  (let [f (fiber/new (fn [] [:got (outer-holder n)]) |:error|)]
    (fiber/resume f)
    (let [r (string "r" n)
          out (fiber/resume f r)
          inner (get (get out 1) 0)]
      (if (and (= (get inner 0) (string "r" n)) (= (get inner 1) (string "h" n))
               (= (get (get out 1) 1) (string "o" n)) (= r (string "r" n)))
        1
        0))))

# ── the payload a parked frame's binding built, read past each seam ─────────
(defn face-dropped [n]
  (let [f (fiber/new (fn [] [:got (payload-outer n)]) |:error|)
        p (fiber/resume f)]
    (if (held? p n) 1 0)))

(defn face-protect [n]
  (let [[ok? p] (protect (payload-outer n))]
    (if (and (not ok?) (held? p n)) 1 0)))

(defn face-cancel [n]
  (let [f (fiber/new (fn [] [:got (payload-outer n)]) |:error|)
        p (fiber/resume f)]
    (fiber/cancel f :gone)
    (if (held? p n) 1 0)))

(defn face-allocs [n]
  (let [[ok? p] (protect (arena/allocs (fn [] (payload-outer n))))]
    (if (and (not ok?) (held? p n)) 1 0)))

# ── a host discards the chain; the value it shares survives ─────────────────
(defn face-eval [n]
  (let [[ok? _] (protect (eval '(oh n) {'oh outer-holder 'n n}))]
    (if (and (not ok?) (= shared "shared-1")) 1 0)))

# The restart answers the get inside the callee, so the result holds every
# frame's binding. A raise that abandons the callee answers the body's call to
# it instead, and this fails on the value before the drive runs.
(let [f (fiber/new (fn [] [:got (outer-holder 1)]) |:error|)]
  (fiber/resume f)
  (assert (= [:got [["r" "h1" "shared-1"] "o1" "shared-1"]] (fiber/resume f "r"))
          "the restart answers the get inside the callee"))

# ── drive: a fresh subject each iteration; an early free faults on it ───────

(def faces
  [face-restart face-dropped face-protect face-cancel face-allocs face-eval])

(defn drive [reps]
  (let [counts @[0 0 0 0 0 0]]
    (var i 0)
    (while (%lt i reps)
      (var k 0)
      (while (%lt k 6)
        (put counts k (+ (get counts k) ((get faces k) i)))
        (assign k (%add k 1)))
      (assign i (%add i 1)))
    (freeze counts)))

(def reps 300)
(let [r (drive reps)]
  (assert (= (get r 0) reps) "restart: a value freed under the result")
  (assert (= (get r 1) reps) "dropped fiber: the payload freed under its reader")
  (assert (= (get r 2) reps) "protect: the payload freed under its reader")
  (assert (= (get r 3) reps) "cancel: the payload freed under its reader")
  (assert (= (get r 4) reps) "arena/allocs: the payload freed under its reader")
  (assert (= (get r 5) reps) "eval: a shared value freed by the discard"))

(println "region-error-park-uaf: ok")
