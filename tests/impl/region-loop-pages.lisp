(elle/epoch 15)
# audited: 2026-10-20
# A loop that allocates nothing claims no region page per iteration, spelled through the wrappers or through intrinsics.
# docs/regions/performance.md
# docs/testing.md
#
# The counter-factual: with the graft off, every (< i n) and (+ i 1) is a call
# into a variadic wrapper, and each call builds its rest list and its letrec
# closure in regions of their own. The counting loops then claim two pages per
# iteration, the CRC-32 step eighteen and the struct scan three, and the loop
# over the wrappers takes about eighteen times the loop over the intrinsics.
# With the graft, every loop reads 0 pages, and the ratio reads about 2: the
# graft binds a written operand with a let, which the loop pays per use
# (#1405).
#
# Each page subject is a run-block of b iterations, read on the page gauge.
# The gauge is deterministic, so a reading is exact, and the ledger in
# tests/ledger/region-loop-pages.lisp pins it. The time subject is the best-of
# ratio of the wrapper loop over the intrinsic loop, on the thread's CPU clock.
#
# The trap: a run-block's bound arrives through a closure call, so nothing
# proves it an int. Each run-block guards it first, as the instrument's own
# run-blocks do, and the guard proves what the loop reads. One subject hides
# its bound in an array on purpose, so its wrapper keeps the guards a proven
# operand would have dropped.

(def r ((import "std/ratchet")))

(defn page-rate [subject run-block]
  (r:drive subject run-block :on [r:pages] :block 400 :min 6))

# ── the control: intrinsics ─────────────────────────────────────────

(defn count-ops [b]
  "b iterations over %lt and %add, which a wrapper loop must cost no more than."
  (when (%not (%int? b)) (error :b-not-int))
  (var i 0)
  (while (%lt i b) (assign i (%add i 1)))
  i)

(page-rate "counting over %-ops" count-ops)

# ── the issue's shapes: a counting loop over the wrappers ───────────

(defn count-wrappers [b]
  "b iterations over < and +, in the run-block itself."
  (when (%not (%int? b)) (error :b-not-int))
  (var i 0)
  (while (< i b) (assign i (+ i 1)))
  i)

(page-rate "counting over wrappers" count-wrappers)

(defn count-to [n]
  "A counting loop in a function of its own, called once per run-block."
  (var i 0)
  (while (< i n) (assign i (+ i 1)))
  i)

(defn count-in-function [b]
  (when (%not (%int? b)) (error :b-not-int))
  (count-to b))

(page-rate "counting in a function" count-in-function)

(def bounds @[0])

(defn count-unproven [b]
  "The bound read back out of an array, which nothing types."
  (when (%not (%int? b)) (error :b-not-int))
  (put bounds 0 b)
  (count-to (get bounds 0)))

(page-rate "counting over an unproven bound" count-unproven)

# A loop over a binding declared outside the run-block: at the top level of a
# direct run that binding is module scope, and under the runner it is a cell
# the run-block captures.
(var cell 0)

(defn count-cell [b]
  (when (%not (%int? b)) (error :b-not-int))
  (assign cell 0)
  (while (< cell b) (assign cell (+ cell 1)))
  cell)

(page-rate "counting over a captured cell" count-cell)

# ── bit operations ──────────────────────────────────────────────────

(defn crc-step [crc byte]
  "One CRC-32 step: fold byte into crc and run its eight bit rounds."
  (var c (bit/xor crc byte))
  (var k 0)
  (while (< k 8)
    (assign
      c
      (if (= (bit/and c 1) 1)
        (bit/xor (bit/shift-right c 1) 0xEDB88320)
        (bit/shift-right c 1)))
    (assign k (+ k 1)))
  c)

(defn crc-run [b]
  (when (%not (%int? b)) (error :b-not-int))
  (var crc 0xFFFFFFFF)
  (var i 0)
  (while (< i b)
    (assign crc (crc-step crc (bit/and i 255)))
    (assign i (+ i 1)))
  crc)

(page-rate "a CRC-32 step per iteration" crc-run)

# ── reads of structs held in a mutable array ────────────────────────

(def frames @[])
(var j 0)
(while (< j 512)
  (push frames {:kind (if (= (bit/and j 1) 0) :a :b) :size j})
  (assign j (+ j 1)))

(defn scan [b]
  "Read one struct per iteration and sum a field of every other one."
  (when (%not (%int? b)) (error :b-not-int))
  (var total 0)
  (var i 0)
  (while (< i b)
    (let [f (get frames (bit/and i 511))]
      (when (= (get f :kind) :a)
        (assign total (+ total (get f :size)))))
    (assign i (+ i 1)))
  total)

(page-rate "a struct read per iteration" scan)

# ── the time a wrapper loop takes against the intrinsic one ─────────

(def n 1000000)

(defn over-wrappers []
  (count-wrappers n))

(defn over-ops []
  (count-ops n))

# Warm both so neither timing pays a first call.
(over-wrappers)
(over-ops)

(r:ratio "counting over wrappers against %-ops" over-wrappers over-ops)

(println "region-loop-pages: ok")
