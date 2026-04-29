(elle/epoch 9)
## lib/resource.lisp — Deterministic resource consumption measurement.
##
## Measures discrete, deterministic counters (allocation counts, intern table
## sizes, etc.) — not wall-clock time. Same program always gives same numbers,
## making these ideal for CI regression detection.
##
## Usage:
##   (def res ((import "std/resource")))
##
##   # Measure a single scenario
##   (def m (res:measure (fn [] (fib 20))))
##   (println (res:report "fib-20" m))
##
##   # Run a suite of named scenarios
##   (res:suite [["fib-20"  (fn [] (fib 20))]
##              ["pair-1k" (fn [] (build-list 1000))]])

(fn []

  ## ── Snapshot ────────────────────────────────────────────────────────

  (defn snapshot []
    "Capture all resource counters at a point in time."
    {:objects (arena/count)
     :bytes (arena/bytes)
     :interns (debug/intern-count)
     :symbols (debug/symbol-count)
     :keywords (debug/keyword-count)})

  ## ── Measure ─────────────────────────────────────────────────────────

  # Calibrate peak overhead once at load time: the measurement
  # infrastructure (arena/allocs pair cells, etc.) contributes a fixed
  # number of transient peak objects that should be subtracted so
  # reported peak reflects only the thunk.
  (def calibration-base (arena/count))
  (arena/reset-peak)
  (arena/allocs (fn [] nil))  # Subtract 1: the calibration allocates its own noop closure which
  # the real measure doesn't (the thunk is passed in, not created).
  (def peak-overhead (- (arena/peak) calibration-base 1))

  (defn measure [thunk]
    "Run thunk, return struct of resource consumption deltas.

     Uses arena/allocs internally for precise heap object counting.
     Peak is adjusted to subtract measurement overhead.
     Before-snapshot uses raw scalars (no struct) to avoid tainting peak."
    (let* [b-objects (arena/count)
           b-bytes (arena/bytes)
           b-interns (debug/intern-count)
           b-symbols (debug/symbol-count)
           b-keywords (debug/keyword-count)
           _ (arena/reset-peak)
           pair (arena/allocs thunk)
           peak (- (arena/peak) b-objects peak-overhead)]
      {:result (first pair)
       :allocs (rest pair)
       :peak peak
       :bytes (- (arena/bytes) b-bytes)
       :interns (- (debug/intern-count) b-interns)
       :symbols (- (debug/symbol-count) b-symbols)
       :keywords (- (debug/keyword-count) b-keywords)}))

  ## ── Report ──────────────────────────────────────────────────────────

  (defn pad-right [s width]
    "Pad string s with spaces to at least width characters."
    (let [n (length s)]
      (if (>= n width)
        s
        (let [buf @""]
          (append buf s)
          (each _ in (range (- width n))
            (append buf " "))
          (freeze buf)))))

  (defn report [name m]
    "Format a measurement as a tab-separated key=value line."
    (let [n (pad-right name 24)]
      (string n "\tallocs=" (m :allocs) "\tpeak=" (m :peak) "\tbytes="
              (m :bytes) "\tinterns=" (m :interns) "\tsymbols=" (m :symbols)
              "\tkeywords=" (m :keywords))))

  ## ── Suite ───────────────────────────────────────────────────────────

  (defn suite [scenarios]
    "Run an array of [name thunk] pairs, print report for each.

     Returns array of [name measurement] results for programmatic use."
    (let [results @[]]
      (each entry in scenarios
        (let* [name (entry 0)
               thunk (entry 1)
               m (measure thunk)]
          (println (report name m))
          (push results [name m])))
      (freeze results)))

  {:snapshot snapshot :measure measure :report report :suite suite})
