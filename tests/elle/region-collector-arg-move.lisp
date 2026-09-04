(elle/epoch 12)
# A fresh heap value handed to a collector parameter over a frame-replacing tail
# call, and what the callee owes for it.
#
# A tail call MOVES its arguments: the caller does not incref them, and the
# release it never runs is the reference the callee's owned-param release
# consumes. A collector parameter — `&`, `&keys`, `&named` — is where that trade
# stops working on its own. The binding names the collected list or struct, so
# the callee's one release frees the COLLECTION and drops the collection's own
# reference to each member. The caller's moved reference is a second one, and
# nothing in the callee names it.
#
# docs/impl/region/mechanism.md § "A collector parameter takes the moved
# reference over itself" owns the argument. This file measures the rate.
#
# ── The counter-factual ──────────────────────────────────────────────
#
# A ceiling on a single drive proves nothing about a rate, so each shape runs at
# two counts and both must read 0. The controls are what tell a real reclamation
# from a dead gauge in the other direction: a positional parameter takes the same
# move through the ordinary owned-param release and must also read 0, and the
# retaining case at the end must read MORE than 0 — a callee that stores its
# collected struct in a module-level sink keeps every value handed to it, so a
# run where these numbers all came back 0 for the wrong reason fails there.
#
# ── The trap ─────────────────────────────────────────────────────────
#
# The call has to be in TAIL position, and the argument has to be allocated by
# the caller in the same call. A non-tail call keeps the caller's release, and a
# value allocated at module scope is one the caller never owned — either one
# reads 0 whatever the collector does, which is why both appear below as their
# own cases rather than as the way the leaking cases are written.

(def small 20)
(def large 60)

# ── The gauge ────────────────────────────────────────────────────────
#
# One run ahead of every window is not counted: the first call through a shape
# pays one-off costs (the callee's code, its template) that are not per call.
# The delta is compared against `ceiling × n` rather than divided, so a
# sub-integer rate cannot floor to 0 and report a leak as reclaimed.

(defn residue [n body]
  (body 0)
  (let [c0 (arena/count)
        r0 (arena/region-count)]
    (def @i 0)
    (while (< i n)
      (body i)
      (assign i (+ i 1)))
    [(- (arena/count) c0) (- (arena/region-count) r0)]))

(defn bounded [label body]
  "`body` leaves nothing behind, at two counts."
  (def @c 0)
  (each n in [small large]
    (let [[objects regions] (residue n body)]
      (assert (= objects 0)
              (string label " n=" n ": " objects " objects retained, expected 0"))
      (assert (= regions 0)
              (string label " n=" n ": " regions " regions retained, expected 0"))
      (println "  " label " n=" n ": " objects " objects, " regions " regions")))
  true)

(defn grows [label body]
  "`body` retains at least one object and one region per run."
  (let [[objects regions] (residue large body)]
    (assert (<= large objects)
            (string "GAUGE DEAD: " label " retained " objects " objects over "
                    large " runs — every bound in this file" " is void"))
    (assert (<= large regions)
            (string "GAUGE DEAD: " label " retained " regions " regions over "
                    large " runs — every bound in this file" " is void"))
    (println "  " label " n=" large ": " objects " objects, " regions " regions"))
  true)

# ── The callees ──────────────────────────────────────────────────────
#
# Each returns its first parameter, so the collected value is dead at the
# callee's exit and every case below differs only in how the value was
# collected.

(defn take-rest [x & xs]
  x)
(defn take-keys [x &keys k]
  x)
(defn take-named [x &named body]
  x)
(defn take-plain [x y]
  x)

# ── The drivers ──────────────────────────────────────────────────────
#
# The call is the driver's whole body, so it is in tail position and the
# argument is allocated inside the same call.

(defn drive-rest []
  (take-rest 1 (bytes "abcdef")))
(defn drive-keys []
  (take-keys 1 :body (bytes "abcdef")))
(defn drive-named []
  (take-named 1 :body (bytes "abcdef")))
(defn drive-plain []
  (take-plain 1 (bytes "abcdef")))

(println "one fresh value moved into a collector...")

(bounded "& rest list" (fn [i] (drive-rest)))
(bounded "&keys struct" (fn [i] (drive-keys)))
(bounded "&named struct" (fn [i] (drive-named)))
(bounded "positional parameter" (fn [i] (drive-plain)))

# ── Several values ───────────────────────────────────────────────────
#
# The release is per collected argument, so a collector holding three fresh
# values owes three.

(defn drive-rest-three []
  (take-rest 1 (bytes "a") (bytes "b") (bytes "c")))

(defn drive-keys-three []
  (take-keys 1 :a (bytes "a") :b (bytes "b") :c (bytes "c")))

(println "several fresh values into one collector...")

(bounded "& rest list, three values" (fn [i] (drive-rest-three)))
(bounded "&keys struct, three values" (fn [i] (drive-keys-three)))

# ── The same value twice: what the release must not read past ────────
#
# One value in two argument positions arrives with ONE moved reference, and a
# fixed slot or an earlier member may already consume it. Releasing per position
# would free a value the callee still holds, so the release is declined for any
# value occurring more than once. That is a deliberate trade in the leak
# direction — never a mis-free — so these shapes retain the one reference nobody
# took over, and the rate below is that conservatism, not a defect in the
# release.
#
# Each case asserts the callee's answer as well as the rate. That is the half
# that cannot be traded away: a release moved past the aliasing check shows up
# here as a wrong answer or a fault, where the rate alone would only get
# smaller and look like an improvement.

(def max-aliased-regions-per-call 1)

(defn at-most [label per-call body]
  "`body` retains no more than `per-call` objects and regions per run."
  (each n in [small large]
    (let [[objects regions] (residue n body)]
      (assert (<= objects (* per-call n))
              (string label " n=" n ": " objects " objects exceeds the "
                      per-call "/call ceiling"))
      (assert (<= regions (* per-call n))
              (string label " n=" n ": " regions " regions exceeds the "
                      per-call "/call ceiling"))
      (println "  " label " n=" n ": " objects " objects, " regions " regions")))
  true)

(defn take-rest-len [x & xs]
  (length xs))
(defn take-keys-a [x &keys k]
  (length (bytes k:a)))

(defn drive-rest-aliased []
  (let [b (bytes "abcdef")]
    (take-rest-len 1 b b)))

(defn drive-keys-aliased []
  (let [b (bytes "abcdef")]
    (take-keys-a 1 :a b :b b)))

(println "one value in two argument positions...")

(at-most "& rest list, aliased value" max-aliased-regions-per-call
         (fn [i]
           (assert (= (drive-rest-aliased) 2)
                   "the callee saw both rest arguments")))
(at-most "&keys struct, aliased value" max-aliased-regions-per-call
         (fn [i]
           (assert (= (drive-keys-aliased) 6)
                   "the callee read the aliased value")))

# ── The value in a collector position is still readable ──────────────
#
# A release that fired while the callee still held the value would show up
# here as a wrong answer rather than as a heap number.

(defn take-named-read [x &named body]
  (length (bytes body)))

(defn drive-named-read []
  (take-named-read 1 :body (bytes "abcdef")))

(println "the collected value survives the call it was collected for...")

(bounded "&named struct, callee reads the value"
         (fn [i]
           (assert (= (drive-named-read) 6)
                   "the callee read its collected value")))

# ── The shapes that read 0 for a different reason ────────────────────
#
# Neither is a control for the release above — each removes the move itself —
# but both are shapes ordinary code writes, and a change that broke either would
# be invisible in the cases above.

(defn drive-keys-nontail []
  (let [r (take-keys 1 :body (bytes "abcdef"))]
    (+ r 0)))

(def module-level-value (bytes "abcdef"))

(defn drive-keys-borrowed []
  (take-keys 1 :body module-level-value))

(println "no move to take over...")

(bounded "&keys struct, call not in tail" (fn [i] (drive-keys-nontail)))
(bounded "&keys struct, value the caller never owned"
         (fn [i] (drive-keys-borrowed)))

# ── The gauge-live gate ──────────────────────────────────────────────
#
# Every bound above passes for two reasons: the release runs, or the gauge is
# dead. A callee that keeps what it collects is unbounded by construction, so it
# must read at least one object and one region per run through the same helper.

(def @sink @[])

(defn take-keys-keeps [x &keys k]
  (push sink k))

(defn drive-keys-kept []
  (take-keys-keeps 1 :body (bytes "abcdef")))

(println "gauge-live gate...")

(grows "&keys struct the callee keeps" (fn [i] (drive-keys-kept)))

(assert (= (length sink) (+ large 1)) "the sink kept every struct it was given")

(println "region collector arg move: every moved reference was taken over")
