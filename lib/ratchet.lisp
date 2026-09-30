(elle/epoch 13)
# audited: 2026-09-30
## lib/ratchet.lisp — measure a shape, print one reading line per subject, and
## judge each reading against the committed ledger.
## docs/ratchet.md
##
## Loaded via: (def r ((import "std/ratchet")))
## Usage:
##   (r:read "unstamped files" :files 12)             — any number, judged
##   (r:delta "residue" (fn [] (send-one)) :on [r:objects r:regions] :n 30)
##   (r:rate "io-drop" probe-io-drop :on [r:objects] :epsilon 0.4)
##   (r:drive "recur-struct" struct-recur)             — the same rate over (run-block b)
##   (r:report)                                        — fail once, naming every problem
##
## The guide is lib/ratchet.md. The estimator is lib/ratchet/estimator.lisp
## and the rows, the judge and the line are lib/ratchet/ledger.lisp.

(fn [& opts]
  (def est ((import "std/ratchet/estimator")))
  (def led ((import "std/ratchet/ledger")))

  # ── gauges ────────────────────────────────────────────────────────
  # One instrument: the dimension it reads, the unit one point of a rate on
  # it carries, the reading itself, and the shape that must move it. The axis
  # rides the gauge rather than the subject because that is where it is a
  # fact: every reading already names the gauge it was taken on.
  (defn gauge [axis unit read &named @disc @floor @epsilon]
    "A gauge: READ answers the current value and allocates nothing. DISC is
     the live-growth probe that proves it, or nil for a gauge nobody proves;
     FLOOR is the per-op rate DISC must reach; EPSILON the half-width a rate on
     this gauge is measured to."
    (default floor 0.5)
    (default epsilon 0.4)
    {:axis axis :unit unit :read read :disc disc :floor floor :epsilon epsilon})

  # The live-growth shapes. A sink that outlives every block keeps whatever it
  # is handed, so the gauge MUST climb about one per op. One sink each: two
  # gauges sharing one would let either probe's ops satisfy the other's
  # floor. Regions never share pages, so each kept struct is a live region
  # holding one page, and the byte gauge climbs a page per op.
  (def @objects-sink @[])
  (def @regions-sink @[])
  (def @bytes-sink @[])
  (def @ids-sink @[])
  (def objects
    (gauge :objects "objects/op" (fn [] (arena/count))
           :disc (fn [j] (push objects-sink {:k j}))))
  (def regions
    (gauge :regions "regions/op" (fn [] (arena/region-count))
           :disc (fn [j] (push regions-sink {:k j}))))
  (def bytes-gauge
    (gauge :bytes "bytes/op" (fn [] (arena/bytes))
           :disc (fn [j] (push bytes-sink {:k j})) :floor 1000.0 :epsilon 512.0))
  # A cons per call mints a region, and a region claims a page. The counter
  # is monotonic, so every op is a claim whether or not the page is recycled.
  (def pages
    (gauge :pages "pages/op" (fn [] (arena/page-claims))
           :disc (fn [j] (pair j j))))
  # A sink of pairs keeps the free list drained, so every later mint has to
  # take a fresh id and the counter climbs.
  (def ids
    (gauge :ids "ids/op" (fn [] (arena/region-ids))
           :disc (fn [j] (push ids-sink (pair j j)))))

  # ── this instrument ───────────────────────────────────────────────
  # The producer is the path this program was started with; a program with
  # none — a form in a worker thread, an -e snippet — prints and judges
  # nothing. The rows are that producer's, from the ledger directory, and nil
  # when no ledger file names the producer: such a program prints and judges
  # nothing too, and the first row written for it is what starts the gate.
  (def root (elle/root))
  (def producer (led:producer-of (sys/argv) root))
  (def ledger-dir (led:ledger-dir root))
  (def rows
    (if (and producer ledger-dir (file/exists? ledger-dir))
      (let [l (get (led:load-dir ledger-dir) producer)]
        (if l (get l :rows) nil))
      nil))
  (def @readings @[])
  (def @void-axes @{})
  (def @proven @{})

  (defn settle! [reading]
    "Judge one reading when there is a ledger to judge it by, print its line,
     and keep it for the report."
    (let [r (if rows (led:judge-with rows void-axes reading) reading)]
      (println (led:render-reading r))
      (push readings r)
      r))

  (defn with-subject [reading subject]
    (put reading :subject subject))

  (defn prove! [g]
    "Drive the gauge's own live-growth shape ahead of its first reading, and
     report it as `<axis> gauge (live-growth)` against its floor."
    (let [axis (get g :axis)]
      (when (and (get g :disc) (not (get proven axis)))
        (put proven axis true)
        (let [rd (get (est:measure (fn [b] (est:run-thunk-block (get g :disc) b))
                                   [g] [(get g :epsilon)] 200 6 60) 0)]
          (settle! (put (put (with-subject rd
                             (string (string axis) " gauge (live-growth)"))
                             :class :growth) :floor (get g :floor)))))))

  # ── readings ──────────────────────────────────────────────────────
  (defn read [subject axis value &named @unit @half]
    "One reading from anywhere: a count a script parsed, the bytes a tool
     reported. UNIT defaults to the axis name and HALF to 0."
    (default unit (string axis))
    (default half 0)
    (settle! {:subject subject :axis axis :value value :half half :unit unit}))

  (defn delta [subject body &named @on @n]
    "Each gauge's change per run of BODY over N runs, after one uncounted run.
     The uncounted run absorbs the one-off cost a window opened straight after
     other work would otherwise read."
    (default on [objects])
    (default n 100)
    (when (%not (%int? n)) (error :n-not-int))
    (each g in on
      (prove! g))
    (body)
    (def reads (map (fn [g] (get g :read)) on))
    (def count (length on))
    # Mutable, so a `put` inside the window stores in place (see the
    # estimator's window).
    (def befores (thaw (map (fn [g] 0) on)))
    (def afters (thaw (map (fn [g] 0) on)))
    (def @i 0)
    (while (%lt i count)
      (put befores i ((get reads i)))
      (assign i (%add i 1)))
    (def @k 0)
    (while (%lt k n)
      (body)
      (assign k (%add k 1)))
    (def @j 0)
    (while (%lt j count)
      (put afters j ((get reads j)))
      (assign j (%add j 1)))
    (let [@out @[]]
      (def @g 0)
      (while (< g count)
        (let [gauge (get on g)
              before (get befores g)
              after (get afters g)]
          (when (%not (%int? before)) (error :gauge-not-integer))
          (when (%not (%int? after)) (error :gauge-not-integer))
          (push out
                (settle! {:subject subject
                          :axis (get gauge :axis)
                          :value (/ (float (%sub after before)) (float n))
                          :half 0
                          :unit (get gauge :unit)
                          :ops n})))
        (assign g (+ g 1)))
      out))

  (defn drive [subject run-block &named @on @epsilon @block @min @max @stable]
    "The adaptive per-op rate of (RUN-BLOCK b), which performs b ops of the
     caller's own shape, on each gauge in ON, measured to EPSILON (each
     gauge's own by default) in blocks of BLOCK ops between MIN and MAX
     blocks. STABLE measures at two block sizes and voids a rate the block
     size moves."
    (default on [objects])
    (default block 100)
    (default min 6)
    (default max 60)
    (default stable false)
    (each g in on
      (prove! g))
    (let [epsilons (map (fn [g] (or epsilon (get g :epsilon))) on)
          rs (if stable
               (est:measure-stable run-block on epsilons block min max)
               (est:measure run-block on epsilons block min max))]
      (map (fn [rd] (settle! (with-subject rd subject))) rs)))

  (defn rate [subject probe &named on epsilon block min max stable]
    "The adaptive per-op rate of (PROBE j), a drive whose run-block calls
     PROBE once per op with the op's index."
    (drive subject (fn [b] (est:run-thunk-block probe b)) :on on
           :epsilon epsilon :block block :min min :max max :stable stable))

  (defn best-of [rounds control subject]
    "The smallest elapsed each thunk reached over ROUNDS alternating rounds.
     Alternating samples the same stretch of machine, and the minimum
     discards a round the scheduler stalled."
    (let [@a nil
          @b nil
          @i 0]
      (while (< i rounds)
        (let [ca (second (time/elapsed control))
              cb (second (time/elapsed subject))]
          (when (or (nil? a) (< ca a)) (assign a ca))
          (when (or (nil? b) (< cb b)) (assign b cb)))
        (assign i (+ i 1)))
      [a b]))

  (defn ratio [subject measured control &named @rounds]
    "The best timing of MEASURED over the best of CONTROL, on the :time axis.
     A starved machine slows both, so the ratio survives what a wall-clock
     bound does not."
    (default rounds 5)
    (let [[c m] (best-of rounds control measured)]
      (settle! {:subject subject :axis :time :value (/ m c) :half 0 :unit "x"})))

  # ── the report ────────────────────────────────────────────────────
  (defn describe [r]
    (string (string (get r :verdict)) "  " (get r :subject) "  "
            (string (get r :axis)) "  " (get r :value) " ±" (get r :half) " "
            (get r :unit) "  " (led:describe-bound (get r :kind) (get r :bound))
            (if (get r :why) (string "  " (get r :why)) "")))

  (defn report []
    "Fail once, naming every reading that is not ok and every row left
     unread. A program with no ledger has nothing to judge, and returns."
    (when rows
      (let [bad (filter (fn [r] (not= (get r :verdict) :ok)) readings)
            missing (led:unread rows readings)
            lines (concat (map describe bad)
                          (map (fn [row]
                                 (string "missing  " (get row :subject) "  "
                                 (string (get row :axis)))) missing))
            msg (string "ratchet: " (length bad) " reading(s) not ok, "
                        (length missing) " row(s) unread, for " producer ":\n  "
                        (string/join lines "\n  "))]
        (assert (and (empty? bad) (empty? missing)) msg)))
    nil)

  {:objects objects
   :regions regions
   :bytes bytes-gauge
   :pages pages
   :ids ids
   :gauge gauge
   :read read
   :delta delta
   :rate rate
   :drive drive
   :stmt-run est:stmt-run
   :ratio ratio
   :report report
   :judge led:judge
   :judge-all led:judge-all
   :parse-row led:parse-row
   :row-key led:row-key
   :readings-in led:readings-in
   :render-reading led:render-reading
   :load-dir led:load-dir
   :ledger-dir led:ledger-dir
   :producer producer
   :rows rows
   :readings readings})
