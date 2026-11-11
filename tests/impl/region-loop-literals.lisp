(elle/epoch 15)
# audited: 2026-11-11
# A loop that compares each item with a string literal, or looks it up in a set or struct literal, builds the literal once per entry.
# docs/impl/hoist.md
# docs/regions/performance.md
#
# The counter-factual: each literal was built again on every pass, into a
# region of its own. A scan of 1000 passes claimed 1000 pages for the string
# literal, and 3000 each for the set and the struct, one for the collection
# and one for each of its two strings. The scan over the string literal took
# about six times the same scan over a global, and the scan over the set
# literal about twelve. An array filled with one literal held a region per
# pass.
#
# Each page subject is a run-block of b scans, read on the page gauge. A scan
# is one entry to a loop of 1000 passes, so a literal built once per entry
# claims one page per value it builds, and the ledger pins that count. Each
# time subject is the best-of ratio of a scan over a literal against the same
# scan over a global, on the thread's CPU clock.
#
# The trap: a top-level function is freed at its last use
# (docs/regions/lifetime.md). Were a function's last call the one between two
# readings of arena/region-count, its closure would leave that window one
# region short. The last section therefore calls each function again after
# its readings. It reads an array of a literal against an array of a keyword
# filled the same way, and the difference is the literal's own region.

(def r ((import "std/ratchet")))

# 1000 one-grapheme strings, none of which any literal below matches.
(def line (->array (string/repeat "x" 1000)))

(def bar "|")
(def bars |"|" "-"|)
(def widths {"|" 1 "-" 2})

# ── the issue's scans: walk the line until an item is the mark ──────

(defn scan-eq-literal [gs]
  (def n (length gs))
  (var i 0)
  (while (and (< i n) (not (= (gs i) "|"))) (assign i (+ i 1)))
  i)

(defn scan-eq-global [gs]
  (def n (length gs))
  (var i 0)
  (while (and (< i n) (not (= (gs i) bar))) (assign i (+ i 1)))
  i)

(defn scan-has-literal [gs]
  (def n (length gs))
  (var i 0)
  (while (and (< i n) (not (has? |"|" "-"| (gs i)))) (assign i (+ i 1)))
  i)

(defn scan-has-global [gs]
  (def n (length gs))
  (var i 0)
  (while (and (< i n) (not (has? bars (gs i)))) (assign i (+ i 1)))
  i)

(defn scan-get-literal [gs]
  (def n (length gs))
  (var i 0)
  (while (and (< i n) (not (get {"|" 1 "-" 2} (gs i)))) (assign i (+ i 1)))
  i)

(defn scan-get-global [gs]
  (def n (length gs))
  (var i 0)
  (while (and (< i n) (not (get widths (gs i)))) (assign i (+ i 1)))
  i)

# ── the pages one entry to a scan claims ────────────────────────────

(defn entries [scan]
  "A run-block of b scans of the line, each one entry to the scan's loop."
  (fn [b]
    (when (%not (%int? b)) (error :b-not-int))
    (var k 0)
    (while (< k b)
      (scan line)
      (assign k (+ k 1)))))

(defn page-rate [subject scan]
  (r:drive subject (entries scan) :on [r:pages] :block 40 :min 6))

(page-rate "a string literal, per scan of 1000 passes" scan-eq-literal)
(page-rate "a global string, per scan of 1000 passes" scan-eq-global)
(page-rate "a set literal, per scan of 1000 passes" scan-has-literal)
(page-rate "a global set, per scan of 1000 passes" scan-has-global)
(page-rate "a struct literal, per scan of 1000 passes" scan-get-literal)
(page-rate "a global struct, per scan of 1000 passes" scan-get-global)

# ── the time a scan over a literal takes against one over a global ──

(defn scans [scan]
  "A thunk that runs scan over the line 200 times."
  (fn []
    (var k 0)
    (while (< k 200)
      (scan line)
      (assign k (+ k 1)))))

(def over-eq-literal (scans scan-eq-literal))
(def over-eq-global (scans scan-eq-global))
(def over-has-literal (scans scan-has-literal))
(def over-has-global (scans scan-has-global))

# Warm each so no timing pays a first call.
(over-eq-literal)
(over-eq-global)
(over-has-literal)
(over-has-global)

(r:ratio "a string literal against a global" over-eq-literal over-eq-global)
(r:ratio "a set literal against a global" over-has-literal over-has-global)

# ── a literal that leaves the loop ──────────────────────────────────

(defn tags [n]
  "An array of n copies of one string literal, filled in a loop."
  (def out @[])
  (var i 0)
  (while (< i n)
    (push out "tag")
    (assign i (+ i 1)))
  out)

(defn keys [n]
  "An array of n copies of one keyword, filled as tags fills its array."
  (def out @[])
  (var i 0)
  (while (< i n)
    (push out :tag)
    (assign i (+ i 1)))
  out)

(def keys-before (arena/region-count))
(def held-keys (keys 100))
(def keys-live (- (arena/region-count) keys-before))
(def keys-count (length held-keys))
(def keys-freed (- (arena/region-count) keys-before))

(def tags-before (arena/region-count))
(def held-tags (tags 100))
(def tags-live (- (arena/region-count) tags-before))
# Each copy is compared, not bound: a binding of a copy would hold the
# literal's region past the array's last use.
(def first-tag? (= (get held-tags 0) "tag"))
(def last-tag? (= (get held-tags 99) "tag"))
(def tags-count (length held-tags))
(def tags-freed (- (arena/region-count) tags-before))

(assert (= keys-count 100) "the control holds a keyword per pass")
(assert (= tags-count 100) "the array holds a copy per pass")
(assert first-tag? "the first copy keeps its value after the loop")
(assert last-tag? "the last copy keeps its value after the loop")
(assert (= (- tags-live keys-live) 1)
        "the 100 copies are one value, in one region beside the array")
(assert (= keys-freed 0) "the control's array is freed at its last use")
(assert (= tags-freed 0) "the literal is freed with the array")

# Each function is read once more, after its readings, so its own closure is
# not freed between them.
(assert (= (length (keys 1)) 1) "keys still answers")
(assert (= (length (tags 1)) 1) "tags still answers")

(println "region-loop-literals: ok")
