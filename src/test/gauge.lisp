(elle/epoch 18)
# audited: 2026-11-23
## elle test — the heap gauges: the list, a worker's readings around its run,
## and the rows each file boundary writes.
## docs/test-gauges.md
##
## A fragment of one module (see store.lisp).

# ── the heap gauges (docs/test-gauges.md) ──────────────────────────
# Every gauge the runner reads, as [kind reader]. Each primitive is Immediate,
# so a call allocates nothing and cannot move the number it reports. One list,
# because the runner's sampling and the workers' readings have to agree about
# which gauges there are. No summary prints them; they are a query over `gauge`
# (docs/test-gauges.md).
#
# The runner reads them on two heaps. Its own heap sees what it does per file:
# the compile, the syntax it holds, and the rows it writes. A worker thread has
# its own VM and heap, so each worker reads the same list around its form's
# tiered call and hands back the readings (gauges-around).
(def heap-gauges
  [["objects" (fn [] (arena/count))] ["regions" (fn [] (arena/region-count))]
   ["pages" (fn [] (arena/page-claims))]
   ["region-frees" (fn [] (arena/region-frees))]
   ["page-frees" (fn [] (arena/page-frees))]
   ["object-frees" (fn [] (arena/object-frees))]
   ["one-page-frees" (fn [] (arena/one-page-frees))]
   ["empty-frees" (fn [] (arena/empty-frees))]
   ["one-object-frees" (fn [] (arena/one-object-frees))]
   ["few-object-frees" (fn [] (arena/few-object-frees))]
   ["many-object-frees" (fn [] (arena/many-object-frees))]
   ["adopts" (fn [] (arena/adopts))]
   ["adopts-into-empty" (fn [] (arena/adopts-into-empty))]
   ["owned" (fn [] (arena/owned))] ["owned-frees" (fn [] (arena/owned-frees))]
   ["owned-free-pages" (fn [] (arena/owned-free-pages))]
   ["owned-free-objects" (fn [] (arena/owned-free-objects))]
   ["owned-one-page-frees" (fn [] (arena/owned-one-page-frees))]
   ["rescues" (fn [] (arena/rescues))]
   ["rescue-survivors" (fn [] (arena/rescue-survivors))]
   ["extracts" (fn [] (arena/extracts))] ["reparents" (fn [] (arena/reparents))]])

# The gauge names alone, in the order a reading and a rendering both take them.
(defn gauge-kinds []
  (map (fn [g] (get g 0)) heap-gauges))

# The reading every window starts from, as kind → value. The runner keeps one
# of these and replaces it at each boundary.
(defn gauge-baseline []
  (let [@prev @{}]
    (each g in heap-gauges
      (put prev (get g 0) ((get g 1))))
    prev))

# ── a worker's readings: the test heap ───────────────────────────────
# Every reading lands in slots allocated before the first, one per gauge. The
# reading loop still costs something, and the same every time, so a reading
# taken right after another measures that cost (gauge-charge).
(defn gauge-slots []
  (let [@slots @[]]
    (each g in heap-gauges
      (push slots 0))
    slots))

(defn read-gauges [slots]
  (var i 0)
  (each g in heap-gauges
    (put slots i ((get g 1)))
    (assign i (+ i 1))))

(defn gauges-around [thunk]
  "Call THUNK and answer [its-value readings]. READINGS are three readings of
   every gauge: two back to back before the call, then one after it. The
   worker computes nothing after the call; the runner does (gauge-charge)."
  (let [warm (gauge-slots)
        before (gauge-slots)
        after (gauge-slots)]
    (read-gauges warm)
    (read-gauges before)
    (let [v (thunk)]
      (read-gauges after)
      [v [warm before after]])))

(defn gauge-charge [readings i]
  "What the call cost gauge I: its change across the call, less the change a
   reading alone makes."
  (let [[warm before after] readings]
    (- (- (get after i) (get before i)) (- (get before i) (get warm i)))))

# The test heap's sums for the file in hand, one per gauge in list order, or
# nil while no run of the file has handed any back. A file whose runs all came
# back empty records no test rows, which says it was not measured.
(def @test-gauge-sums nil)

(defn add-test-gauges [readings]
  "Add one run's charges to the file's sums."
  (when (= test-gauge-sums nil) (assign test-gauge-sums (gauge-slots)))
  (var i 0)
  (each g in heap-gauges
    (put test-gauge-sums i (+ (get test-gauge-sums i) (gauge-charge readings i)))
    (assign i (+ i 1))))

(defn test-gauge-mark [conn run-id file]
  "Write the file's test-heap sums, one row per gauge, and start the next file
   empty. A worker heap lives for one run, so a row has no reading to chain."
  (when test-gauge-sums
    (let [@i 0]
      (each kind in (gauge-kinds)
        (sqlite:exec conn
                     "INSERT INTO gauge (run_id, file, heap, kind, delta, reading) VALUES (?1,?2,'test',?3,?4,NULL)"
                     [run-id (string file) kind (get test-gauge-sums i)])
        (assign i (+ i 1))))
    (assign test-gauge-sums nil))
  nil)

# One boundary: charge each gauge's change since the previous boundary to FILE,
# and leave this reading as the next window's start.
#
# ONE reading per boundary, not a before/after pair around each file. A pair
# leaves the rows written between them charged to nobody, and that gap is where
# the runner's own work lives. With one reading every object the runner
# allocates lands in exactly one file's window, and the readings chain: a
# file's `reading` plus the next file's `delta` is the next file's `reading`.
(defn gauge-mark [conn run-id prev file]
  (each g in heap-gauges
    (let [kind (get g 0)
          now ((get g 1))]
      (sqlite:exec conn
                   "INSERT INTO gauge (run_id, file, heap, kind, delta, reading) VALUES (?1,?2,'runner',?3,?4,?5)"
                   [run-id (string file) kind (- now (get prev kind)) now])
      (put prev kind now)))
  nil)
