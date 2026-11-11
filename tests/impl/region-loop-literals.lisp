(elle/epoch 15)
# audited: 2026-11-11
# A loop that compares each item with a string literal, or looks it up in a set or struct literal, claims no page per iteration.
# docs/impl/hoist.md
# docs/regions/performance.md
#
# The counter-factual: each literal was built again on every pass, into a
# region of its own. The string loop claimed one page per iteration, and the
# set and struct loops three each, one for the collection and one for each
# string in it. The scan over the string literal took about six times the same
# scan over a global, and the scan over the set literal about twelve. The
# array the last section fills held a region per pass, 100 in all.
#
# Each page subject is a run-block of b iterations, read on the page gauge,
# and tests/ledger/region-loop-literals.lisp pins each at 0. Each time subject
# is the best-of ratio of a literal loop over the same loop over a global, on
# the thread's CPU clock.
#
# The trap: a run-block's bound arrives through a closure call, so nothing
# proves it an int. Each run-block guards it first, as
# tests/impl/region-loop-pages.lisp does, so the loop compiles over a proven
# counter and the literal is the only allocation left in it.

(def r ((import "std/ratchet")))

(defn page-rate [subject run-block]
  (r:drive subject run-block :on [r:pages] :block 400 :min 6))

# 256 one-grapheme strings, none of which any literal below matches.
(def items (->array (string/repeat "x" 256)))

(def bar "|")
(def bars |"|" "-"|)
(def widths {"|" 1 "-" 2})

# ── a string literal ────────────────────────────────────────────────

(defn eq-literal [b]
  "b iterations, each comparing an item with the literal \"|\"."
  (when (%not (%int? b)) (error :b-not-int))
  (var hits 0)
  (var i 0)
  (while (< i b)
    (when (= (get items (bit/and i 255)) "|") (assign hits (+ hits 1)))
    (assign i (+ i 1)))
  hits)

(defn eq-global [b]
  "eq-literal, with the literal bound to a global."
  (when (%not (%int? b)) (error :b-not-int))
  (var hits 0)
  (var i 0)
  (while (< i b)
    (when (= (get items (bit/and i 255)) bar) (assign hits (+ hits 1)))
    (assign i (+ i 1)))
  hits)

(page-rate "a string literal per iteration" eq-literal)
(page-rate "a global string per iteration" eq-global)

# ── a set literal ───────────────────────────────────────────────────

(defn has-literal [b]
  "b iterations, each looking an item up in the literal |\"|\" \"-\"|."
  (when (%not (%int? b)) (error :b-not-int))
  (var hits 0)
  (var i 0)
  (while (< i b)
    (when (has? |"|" "-"| (get items (bit/and i 255))) (assign hits (+ hits 1)))
    (assign i (+ i 1)))
  hits)

(defn has-global [b]
  "has-literal, with the set bound to a global."
  (when (%not (%int? b)) (error :b-not-int))
  (var hits 0)
  (var i 0)
  (while (< i b)
    (when (has? bars (get items (bit/and i 255))) (assign hits (+ hits 1)))
    (assign i (+ i 1)))
  hits)

(page-rate "a set literal per iteration" has-literal)
(page-rate "a global set per iteration" has-global)

# ── a struct literal ────────────────────────────────────────────────

(defn get-literal [b]
  "b iterations, each looking an item up in the literal {\"|\" 1 \"-\" 2}."
  (when (%not (%int? b)) (error :b-not-int))
  (var total 0)
  (var i 0)
  (while (< i b)
    (assign total (+ total (get {"|" 1 "-" 2} (get items (bit/and i 255)) 0)))
    (assign i (+ i 1)))
  total)

(defn get-global [b]
  "get-literal, with the struct bound to a global."
  (when (%not (%int? b)) (error :b-not-int))
  (var total 0)
  (var i 0)
  (while (< i b)
    (assign total (+ total (get widths (get items (bit/and i 255)) 0)))
    (assign i (+ i 1)))
  total)

(page-rate "a struct literal per iteration" get-literal)
(page-rate "a global struct per iteration" get-global)

# ── the time a literal loop takes against the global one ─────────────

# The issue's scan: walk a line of graphemes until one is the mark, which none
# is. Each pass reads one item and makes one comparison, so the literal is most
# of what a pass would cost if it were built on every pass.
(def line (->array (string/repeat "x" 1000)))

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

# The array and the one string it holds n times are the two regions the call
# leaves. Built per pass, the string would leave a region per element.
(defn tags [n]
  "An array of n copies of one literal, filled in a loop."
  (def out @[])
  (var i 0)
  (while (< i n)
    (push out "tag")
    (assign i (+ i 1)))
  out)

(def before (arena/region-count))
(def held (tags 100))
(def live (- (arena/region-count) before))
(def first-tag (get held 0))
(def last-tag (get held 99))
(def held-count (length held))
(def freed (- (arena/region-count) before))

(assert (= held-count 100) "the array holds a copy per pass")
(assert (= first-tag "tag") "the first copy keeps its value after the loop")
(assert (= last-tag "tag") "the last copy keeps its value after the loop")
(assert (= live 2) "the array and the one literal it holds are the live regions")
(assert (= freed 0) "both are freed at the array's last use")

(println "region-loop-literals: ok")
