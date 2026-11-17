(elle/epoch 15)
# audited: 2026-11-17
# A collector parameter takes over the reference a frame-replacing tail call moves into it.
# docs/impl/region/relocate.md
#
# A tail call MOVES its arguments: the caller does not incref them, and the
# release it never runs is the reference the callee's owned-param release
# consumes. A collector parameter — `&`, `&keys`, `&named` — is where that trade
# stops working on its own. The binding names the collected list or struct, so
# the callee's one release frees the COLLECTION and drops the collection's own
# reference to each member. The caller's moved reference is a second one, and
# nothing in the callee names it.
#
# This file measures the rate, per call, on the object and region counts,
# against the rows of tests/ledger/region-collector-arg-move.lisp.
#
# ── The counter-factual ──────────────────────────────────────────────
#
# Every rate runs the block-size check, so a reading of 0 is a per-call rate
# rather than a per-block artifact. The controls tell a real reclamation from a
# dead gauge in the other direction. A positional parameter takes the same move
# through the ordinary owned-param release, and must also read 0. The retaining
# case at the end must read one per call: its callee stores its collected struct
# in a module-level sink, which keeps every value handed to it. So a run where
# these numbers all came back 0 for the wrong reason fails there.
#
# ── The trap ─────────────────────────────────────────────────────────
#
# The call has to be in TAIL position, and the argument has to be allocated by
# the caller in the same call. A non-tail call keeps the caller's release, and
# the caller never owned a value allocated at module scope. Either one reads 0
# whatever the collector does, so both appear below as cases of their own rather
# than as the way the leaking cases are written.

(def r ((import "std/ratchet")))

(defn moved-rate [subject probe]
  (r:rate subject probe :on [r:objects r:regions r:allocs] :stable true
          :block 20 :min 4 :max 8))

# Six bytes the op's index decides, built where the macro is written. A
# `(bytes "abcdef")` of a constant string is built once per unit
# (docs/impl/hoist.md), and a value the caller never built reads 0 whatever the
# collector does. The allocation rows hold every driver to building its value.
(defmacro fresh-bytes [j]
  `(bytes 97 98 99 100 101 (bit/and ,j 255)))

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

(defn drive-rest [j]
  (take-rest 1 (fresh-bytes j)))
(defn drive-keys [j]
  (take-keys 1 :body (fresh-bytes j)))
(defn drive-named [j]
  (take-named 1 :body (fresh-bytes j)))
(defn drive-plain [j]
  (take-plain 1 (fresh-bytes j)))

(println "one fresh value moved into a collector...")

(moved-rate "& rest list" (fn [j] (drive-rest j)))
(moved-rate "&keys struct" (fn [j] (drive-keys j)))
(moved-rate "&named struct" (fn [j] (drive-named j)))
(moved-rate "positional parameter" (fn [j] (drive-plain j)))

# ── Several values ───────────────────────────────────────────────────
#
# The release is per collected argument, so a collector holding three fresh
# values owes three.

(defn drive-rest-three [j]
  (take-rest 1 (fresh-bytes j) (fresh-bytes j) (fresh-bytes j)))

(defn drive-keys-three [j]
  (take-keys 1 :a (fresh-bytes j) :b (fresh-bytes j) :c (fresh-bytes j)))

(println "several fresh values into one collector...")

(moved-rate "& rest list, three values" (fn [j] (drive-rest-three j)))
(moved-rate "&keys struct, three values" (fn [j] (drive-keys-three j)))

# ── The same value twice: what the release must not read past ────────
#
# One value in two argument positions arrives with ONE moved reference, and a
# fixed slot or an earlier member may already consume it. Releasing per position
# would free a value the callee still holds, so the release is declined for any
# value occurring more than once. That is a deliberate trade in the leak
# direction, never a mis-free. So these shapes retain the one reference nobody
# took over, and the rate is that conservatism, not a defect in the release.
#
# Each case asserts the callee's answer as well as the rate. That half cannot be
# traded away. A release moved past the aliasing check shows up here as a wrong
# answer or a fault, where the rate alone would only get
# smaller and look like an improvement.

(defn take-rest-len [x & xs]
  (length xs))
(defn take-keys-a [x &keys k]
  (length (bytes k:a)))

(defn drive-rest-aliased [j]
  (let [b (fresh-bytes j)]
    (take-rest-len 1 b b)))

(defn drive-keys-aliased [j]
  (let [b (fresh-bytes j)]
    (take-keys-a 1 :a b :b b)))

(println "one value in two argument positions...")

(moved-rate "& rest list, aliased value"
            (fn [j]
              (assert (= (drive-rest-aliased j) 2)
                      "the callee saw both rest arguments")))
(moved-rate "&keys struct, aliased value"
            (fn [j]
              (assert (= (drive-keys-aliased j) 6)
                      "the callee read the aliased value")))

# ── The value in a collector position is still readable ──────────────
#
# A release that fired while the callee still held the value would show up
# here as a wrong answer rather than as a heap number.

(defn take-named-read [x &named body]
  (length (bytes body)))

(defn drive-named-read [j]
  (take-named-read 1 :body (fresh-bytes j)))

(println "the collected value survives the call it was collected for...")

(moved-rate "&named struct, callee reads the value"
            (fn [j]
              (assert (= (drive-named-read j) 6)
                      "the callee read its collected value")))

# ── The shapes that read 0 for a different reason ────────────────────
#
# Neither is a control for the release above — each removes the move itself —
# but both are shapes ordinary code writes, and a change that broke either would
# be invisible in the cases above.

(defn drive-keys-nontail [j]
  (let [r (take-keys 1 :body (fresh-bytes j))]
    (+ r 0)))

(def module-level-value (bytes "abcdef"))

(defn drive-keys-borrowed []
  (take-keys 1 :body module-level-value))

(println "no move to take over...")

(moved-rate "&keys struct, call not in tail" (fn [j] (drive-keys-nontail j)))
(moved-rate "&keys struct, value the caller never owned"
            (fn [j] (drive-keys-borrowed)))

# ── The callee that keeps what it collects ───────────────────────────
#
# Unbounded by construction: its rows are growth floors at one per call, so a
# run in which it reads less voids every rate on its axis.

(def @sink @[])

(defn take-keys-keeps [x &keys k]
  (push sink k))

(defn drive-keys-kept [j]
  (take-keys-keeps 1 :body (fresh-bytes j)))

(println "a callee that keeps its collected struct...")

(moved-rate "&keys struct the callee keeps" (fn [j] (drive-keys-kept j)))

(println "region collector arg move: every moved reference was taken over")
