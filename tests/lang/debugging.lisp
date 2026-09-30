(elle/epoch 14)
# audited: 2026-09-30
# debug/print and debug/trace write a value to stderr and answer it unchanged; debug/memory reads the process's memory use.
# src/primitives/debug.rs

# ── debug/print answers its argument ────────────────────────────────

(assert (= (debug/print 42) 42) "debug/print answers an int unchanged")
(assert (= (debug/print "hello") "hello")
        "debug/print answers a string unchanged")
(assert (= (debug/print (+ 1 2)) 3)
        "debug/print answers the value of an expression")
(assert (= (debug/print (list (list 1 2) (list 3 4)))
           (list (list 1 2) (list 3 4)))
        "debug/print answers a nested list unchanged")
(let [arr @[1 2 3]]
  (assert (identical? (debug/print arr) arr)
          "debug/print answers the very array it was given"))

(let [[ok? _] (protect ((fn () (eval '(debug/print)))))]
  (assert (not ok?) "debug/print with no arguments fails"))
(let [[ok? _] (protect ((fn () (eval '(debug/print 1 2)))))]
  (assert (not ok?) "debug/print with two arguments fails"))

# ── debug/trace answers its second argument ─────────────────────────

(assert (= (debug/trace "label" 42) 42)
        "debug/trace answers its second argument")
(assert (= (debug/trace "computation" (+ 5 3)) 8)
        "debug/trace answers the value of an expression")
(assert (= (debug/trace 'step 8) 8) "debug/trace takes a symbol as its label")

(let [[ok? _] (protect ((fn () (eval '(debug/trace)))))]
  (assert (not ok?) "debug/trace with no arguments fails"))
(let [[ok? _] (protect ((fn () (eval '(debug/trace "label")))))]
  (assert (not ok?) "debug/trace with one argument fails"))
(let [[ok? _] (protect ((fn () (eval '(debug/trace "a" "b" "c")))))]
  (assert (not ok?) "debug/trace with three arguments fails"))

(let [[ok? err] (protect (debug/trace 42 100))]
  (assert (and (not ok?) (= (get err :error) :type-error))
          "debug/trace refuses an int label"))
(let [[ok? err] (protect (debug/trace nil 100))]
  (assert (and (not ok?) (= (get err :error) :type-error))
          "debug/trace refuses a nil label"))

# ── debug/memory reads two byte counts ──────────────────────────────

(let [reading (debug/memory)]
  (assert (list? reading) "debug/memory answers a list")
  (assert (= (length reading) 2) "debug/memory answers two counts")
  (let [rss (first reading)
        virtual (second reading)]
    (assert (and (int? rss) (>= rss 0)) "the resident size is a byte count")
    (assert (and (int? virtual) (>= virtual 0))
            "the virtual size is a byte count")))

# ── The older names ─────────────────────────────────────────────────

(assert (= debug-print debug/print) "debug-print names debug/print")
(assert (= trace debug/trace) "trace names debug/trace")
(assert (= memory-usage debug/memory) "memory-usage names debug/memory")
