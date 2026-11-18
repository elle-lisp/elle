(elle/epoch 15)
# audited: 2026-11-17
# A predicate that compares its argument with a string literal, or looks it up in a set literal, claims no page per call.
# docs/impl/hoist.md
# docs/regions/performance.md
#
# The counter-factual: each call built its literal again, into a region of
# its own. A walk of 1000 calls claimed 1000 pages for the string literal,
# and 3000 for the set literal, one for the set and one for each of its two
# strings. The predicate over the string literal took about four times the
# one over a global, and the one over the set literal about six. An array
# filled by a function that returns a literal held a region per element.
#
# Each function reaches its loop through a mutable table, so the compiler
# cannot see which function the loop calls. A graft would otherwise put the
# function's body, and its literal, inside the loop.
#
# Each page subject is a run-block of b walks, read on the page gauge. A walk
# is 1000 calls of the predicate, so a literal built per call claims a page
# per call, and the ledger pins the count at none. Each time subject is the
# best-of ratio of walks over a literal against walks over a global.
#
# The trap: a top-level function is freed at its last use
# (docs/regions/lifetime.md). The last section therefore reads each table,
# and calls fill, again after its readings.

(def r ((import "std/ratchet")))

# 1000 one-grapheme strings, none of which any literal below matches.
(def line (->array (string/repeat "x" 1000)))

(def bar "|")
(def bars |"|" "-"|)

(def preds
  @{:eq-literal (fn [g] (= g "|"))
    :eq-global (fn [g] (= g bar))
    :has-literal (fn [g] (has? |"|" "-"| g))
    :has-global (fn [g] (has? bars g))})

(defn walk [gs p?]
  "The index of the first item of gs that p? accepts, or the length of gs."
  (def n (length gs))
  (var i 0)
  (while (and (< i n) (not (p? (gs i)))) (assign i (+ i 1)))
  i)

# ── the pages one walk claims ───────────────────────────────────────

(defn walks [name]
  "A run-block of b walks of the line with the predicate name names."
  (fn [b]
    (when (%not (%int? b)) (error :b-not-int))
    (def p? (get preds name))
    (var k 0)
    (while (< k b)
      (walk line p?)
      (assign k (+ k 1)))))

(defn page-rate [subject name]
  (r:drive subject (walks name) :on [r:pages] :block 40 :min 6))

(page-rate "a string literal, per walk of 1000 calls" :eq-literal)
(page-rate "a global string, per walk of 1000 calls" :eq-global)
(page-rate "a set literal, per walk of 1000 calls" :has-literal)
(page-rate "a global set, per walk of 1000 calls" :has-global)

# ── the time a walk over a literal takes against one over a global ──

(defn timed [name]
  "A thunk that walks the line 200 times with the predicate name names."
  (def p? (get preds name))
  (fn []
    (var k 0)
    (while (< k 200)
      (walk line p?)
      (assign k (+ k 1)))))

(def over-eq-literal (timed :eq-literal))
(def over-eq-global (timed :eq-global))
(def over-has-literal (timed :has-literal))
(def over-has-global (timed :has-global))

# Warm each so no timing pays a first call.
(over-eq-literal)
(over-eq-global)
(over-has-literal)
(over-has-global)

(r:ratio "a string literal against a global" over-eq-literal over-eq-global)
(r:ratio "a set literal against a global" over-has-literal over-has-global)

# ── a literal that a function returns ───────────────────────────────

(def makers @{:tag (fn [] "tag") :key (fn [] :tag)})

(defn fill [n name]
  "An array of the n values the maker name names answers."
  (def make (get makers name))
  (def out @[])
  (var i 0)
  (while (< i n)
    (push out (make))
    (assign i (+ i 1)))
  out)

(def keys-before (arena/region-count))
(def held-keys (fill 100 :key))
(def keys-live (- (arena/region-count) keys-before))
(def keys-count (length held-keys))
(def keys-freed (- (arena/region-count) keys-before))

(def tags-before (arena/region-count))
(def held-tags (fill 100 :tag))
(def tags-live (- (arena/region-count) tags-before))
# Each copy is compared, not bound: a binding of a copy would hold the
# literal's region past the array's last use.
(def first-tag? (= (get held-tags 0) "tag"))
(def last-tag? (= (get held-tags 99) "tag"))
(def tags-count (length held-tags))
(def tags-freed (- (arena/region-count) tags-before))

(assert (= keys-count 100) "the control holds a keyword per call")
(assert (= tags-count 100) "the array holds a copy per call")
(assert first-tag? "the first copy keeps its value after its call")
(assert last-tag? "the last copy keeps its value after its call")
(assert (= tags-live keys-live)
        "the 100 copies are the one value the unit built, so only the array is new")
(assert (= keys-freed 0) "the control's array is freed at its last use")
(assert (= tags-freed 0) "the array of copies is freed at its last use")

# Each table is read, and fill called, once more after its readings, so no
# closure they hold or name is freed between them.
(assert (= ((get makers :tag)) "tag") "the maker still answers its literal")
(assert (= (length (fill 1 :key)) 1) "fill still fills")
(assert (= (walk line (get preds :eq-literal)) 1000)
        "the predicate still answers")

(println "region-fn-literals: ok")
