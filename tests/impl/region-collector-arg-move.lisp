(elle/epoch 14)
# audited: 2026-10-05
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
# reference over itself" owns the argument. This file measures the rate, per
# call, on the object and region counts, against the rows of
# tests/ledger/region-collector-arg-move.lisp.
#
# ── The counter-factual ──────────────────────────────────────────────
#
# Every rate runs the block-size check, so a reading of 0 is a per-call rate
# rather than a per-block artifact. The controls are what tell a real
# reclamation from a dead gauge in the other direction: a positional parameter
# takes the same move through the ordinary owned-param release and must also
# read 0, and the retaining case at the end must read one per call — a callee
# that stores its collected struct in a module-level sink keeps every value
# handed to it, so a run where these numbers all came back 0 for the wrong
# reason fails there.
#
# ── The trap ─────────────────────────────────────────────────────────
#
# The call has to be in TAIL position, and the argument has to be allocated by
# the caller in the same call. A non-tail call keeps the caller's release, and a
# value allocated at module scope is one the caller never owned — either one
# reads 0 whatever the collector does, which is why both appear below as their
# own cases rather than as the way the leaking cases are written.

(def r ((import "std/ratchet")))

(defn moved-rate [subject probe]
  (r:rate subject probe :on [r:objects r:regions] :stable true :block 20 :min 4
          :max 8))

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

(moved-rate "& rest list" (fn [j] (drive-rest)))
(moved-rate "&keys struct" (fn [j] (drive-keys)))
(moved-rate "&named struct" (fn [j] (drive-named)))
(moved-rate "positional parameter" (fn [j] (drive-plain)))

# ── Several values ───────────────────────────────────────────────────
#
# The release is per collected argument, so a collector holding three fresh
# values owes three.

(defn drive-rest-three []
  (take-rest 1 (bytes "a") (bytes "b") (bytes "c")))

(defn drive-keys-three []
  (take-keys 1 :a (bytes "a") :b (bytes "b") :c (bytes "c")))

(println "several fresh values into one collector...")

(moved-rate "& rest list, three values" (fn [j] (drive-rest-three)))
(moved-rate "&keys struct, three values" (fn [j] (drive-keys-three)))

# ── The same value twice: what the release must not read past ────────
#
# One value in two argument positions arrives with ONE moved reference, and a
# fixed slot or an earlier member may already consume it. Releasing per position
# would free a value the callee still holds, so the release is declined for any
# value occurring more than once. That is a deliberate trade in the leak
# direction — never a mis-free — so these shapes retain the one reference nobody
# took over, and the rate is that conservatism, not a defect in the release.
#
# Each case asserts the callee's answer as well as the rate. That is the half
# that cannot be traded away: a release moved past the aliasing check shows up
# here as a wrong answer or a fault, where the rate alone would only get
# smaller and look like an improvement.

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

(moved-rate "& rest list, aliased value"
            (fn [j]
              (assert (= (drive-rest-aliased) 2)
                      "the callee saw both rest arguments")))
(moved-rate "&keys struct, aliased value"
            (fn [j]
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

(moved-rate "&named struct, callee reads the value"
            (fn [j]
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

(moved-rate "&keys struct, call not in tail" (fn [j] (drive-keys-nontail)))
(moved-rate "&keys struct, value the caller never owned"
            (fn [j] (drive-keys-borrowed)))

# ── The callee that keeps what it collects ───────────────────────────
#
# Unbounded by construction: its rows are growth floors at one per call, so a
# run in which it reads less voids every rate on its axis.

(def @sink @[])

(defn take-keys-keeps [x &keys k]
  (push sink k))

(defn drive-keys-kept []
  (take-keys-keeps 1 :body (bytes "abcdef")))

(println "a callee that keeps its collected struct...")

(moved-rate "&keys struct the callee keeps" (fn [j] (drive-keys-kept)))

(println "region collector arg move: every moved reference was taken over")
