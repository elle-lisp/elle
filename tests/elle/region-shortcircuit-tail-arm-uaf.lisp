(elle/epoch 12)
# audited: 2026-09-08
# Soundness complement of region-shortcircuit-tail-arm.lisp: the release an
# `and`/`or` merge replicates into its last operand's arm must free nothing that
# arm still reads (docs/impl/region/replicate.md).
#
# The replica fires where no release fired before, so it owes a count argument.
# Four ways it could be wrong, and each faults here: it frees the region the arm's
# call is MOVING into its callee, whose owned-param release then drops a reference
# already gone; it frees a region the callee reaches through its CAPTURED
# environment; it frees a region the callee hands BACK, which the caller reads
# after the mint; or it runs a second time on the NATIVE fall-through, where the
# arm's call keeps the frame and the merge's own copy runs too.
#
# Every witness reads the subject's HEAP contents after the arm's call, through a
# chain long enough that an over-early free faults rather than reading stale but
# still-mapped bytes. A fresh subject per iteration keeps region ids churning, so
# a freed region is recycled under its reader.

(def @sink @[])

# ── callees ──────────────────────────────────────────────────────────────────

(defn nought []
  0)

(defn read-len [a]
  (length a:k))

# ── witnesses ────────────────────────────────────────────────────────────────

# (a) the arm MOVES `x` into its callee, whose owned-param release is what frees
# it. A replica ahead of the call drops that reference first, and the callee's
# read of `a:k` finds a freed page.
(defn w-moved-inner [x t]
  (or t (read-len x)))
(defn w-moved [i]
  (let [r (w-moved-inner {:k (string "m" i "-long")} false)]
    (if r 1 0)))

# (b) the arm's callee reaches `x` through its CAPTURED environment, which no
# argument names. The funnel counted that hold when the env was built, so the
# replica must leave the callee's read standing.
(defn w-captured-inner [x t]
  (let [g (fn [] (length x:k))]
    (or t (g))))
(defn w-captured [i]
  (let [r (w-captured-inner {:k (string "c" i "-long")} false)]
    (if r 1 0)))

# (c) the arm's callee hands `x` BACK, so the caller's reference is minted by the
# callee's own `Return` — after the relocated release has run. The counted env
# edge is what holds the region across that gap.
(defn w-handback-inner [x t]
  (let [g (fn [] x)]
    (or t (g))))
(defn w-handback [i]
  (let [r (w-handback-inner {:k (string "h" i "-long")} false)]
    (length r:k)))

# (d) `x` escapes into a container that outlives the frame before the branch, and
# is read back out afterwards. The store's incref is what the replica must not
# take below zero.
(defn w-store-inner [x t]
  (push sink x)
  (or t (nought)))
(defn w-store [i]
  (w-store-inner {:k (string "s" i "-long")} false)
  (length (get (get sink (%sub (length sink) 1)) :k)))

# (e) a closure that ESCAPES captured `x`; calling it after the frame is gone must
# still reach the captured region.
(defn w-escape-inner [x t]
  (let [g (fn [] (length x:k))]
    (push sink g)
    (or t (nought))))
(defn w-escape [i]
  (w-escape-inner {:k (string "e" i "-long")} false)
  (let [g (get sink (%sub (length sink) 1))]
    (if (g) 1 0)))

# (f) the arm's callee is a NATIVE, which pushes no frame — so the fall-through
# runs the replica AND reaches the merge, where the same release stands. Two
# copies on one path count once only because the value route nil-stamps the slot
# it read; without the stamp the second decref names a freed and recycled region.
(defn w-native-inner [x t]
  (or t (length "abcdef")))
(defn w-native [i]
  (let [x {:k (string "n" i "-long")}]
    (w-native-inner x false)
    (length x:k)))

# ── control: the same read with no short circuit (harness sanity) ─────────────
(defn c-plain [i]
  (let [x {:k (string "p" i "-long")}]
    (length x:k)))

# ── drive: fresh subject each iteration; an over-early free faults on the read ─
(var i 0)
(var a 0)
(var b 0)
(var c 0)
(var d 0)
(var e 0)
(var f 0)
(var k 0)
(while (%lt i 3000)
  (assign a (w-moved i))
  (assign b (w-captured i))
  (assign c (w-handback i))
  (assign d (w-store i))
  (assign e (w-escape i))
  (assign f (w-native i))
  (assign k (c-plain i))
  # The sink is a module-level container by design (witnesses d and e store into
  # it); drain it so the driver's own retention stays flat.
  (assign sink @[])
  (assign i (%add i 1)))

(assert (%gt k 0) "control: plain struct read mis-read (harness broken)")

(assert (%gt a 0) "the region the arm moved into its callee was freed first")
(assert (%gt b 0)
        "the region the arm's callee captured was freed before the call")
(assert (%gt c 0)
        "the region the arm's callee handed back was freed under the caller")
(assert (%gt d 0) "a value stored into a longer-lived container was freed")
(assert (%gt e 0) "a value the escaping closure captured was freed")
(assert (%gt f 0) "the replica and the merge's own release both ran on one path")

(println "region-shortcircuit-tail-arm-uaf: ok")
