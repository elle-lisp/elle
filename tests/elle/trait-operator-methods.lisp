(elle/epoch 12)
# audited: 2026-09-20
# tests/elle/trait-operator-methods.lisp — a trait method named for an
# operator overrides that operator.
# docs/traits.md
#
## A per-operator trait method overrides the operator it is named for.
##
## The counter-factual: every operator here ended in a hardcoded type cascade,
## so a with-traits collection that answered `first` and `length` raised
## :type-error from `map`. A test that only asserted "no error" would pass the
## moment map returned anything at all, so each assertion pins a sentinel that
## nothing but the method itself can produce — and pins the arguments the
## method receives, which is where an argument-order mistake shows up.

## Half the methods live in :Sequence and half in :Collection, because the
## lookup reads both protocols. Each one reports the arguments it was handed.
(def probe
  (with-traits {:tag :probe}
               @{:Sequence {:map (fn [self f] [:map (f 2)])
                            :filter (fn [self p] [:filter (p 2)])
                            :take (fn [self n] [:take n])
                            :drop (fn [self n] [:drop n])
                            :last (fn [self] :last)
                            :butlast (fn [self] :butlast)
                            :reverse (fn [self] :reverse)
                            :flatten (fn [self] :flatten)
                            :distinct (fn [self] :distinct)
                            :interpose (fn [self sep] [:interpose sep])
                            :partition (fn [self n] [:partition n])
                            :map-indexed (fn [self f] [:map-indexed (f 1 2)])
                            :mapcat (fn [self f] [:mapcat (f 2)])
                            :take-while (fn [self p] [:take-while (p 2)])
                            :drop-while (fn [self p] [:drop-while (p 2)])
                            :sort-by (fn [self k] [:sort-by (k 2)])
                            :sort-with (fn [self c] [:sort-with (c 1 2)])}
                 :Collection {:fold (fn [self f init] [:fold (f init 2)])
                              :count (fn [self p] [:count (p 2)])
                              :find-index (fn [self p] [:find-index (p 2)])
                              :find (fn [self p] [:find (p 2)])
                              :any? (fn [self p] [:any? (p 2)])
                              :all? (fn [self p] [:all? (p 2)])
                              :update (fn [self key f] [:update key (f 5)])}}))

(defn ten [x]
  (* x 10))

# ============================================================================
# The method wins, and it is handed (self . the operator's own arguments)
# ============================================================================

(assert (= (map ten probe) [:map 20]) "map calls its :map method")
(assert (= (filter ten probe) [:filter 20]) "filter calls its :filter method")
(assert (= (keep ten probe) [:filter 20]) "keep is filter, so it shares :filter")
(assert (= (take 3 probe) [:take 3]) "take calls its :take method")
(assert (= (drop 3 probe) [:drop 3]) "drop calls its :drop method")
(assert (= (last probe) :last) "last calls its :last method")
(assert (= (butlast probe) :butlast) "butlast calls its :butlast method")
(assert (= (reverse probe) :reverse) "reverse calls its :reverse method")
(assert (= (flatten probe) :flatten) "flatten calls its :flatten method")
(assert (= (distinct probe) :distinct) "distinct calls its :distinct method")
(assert (= (interpose :sep probe) [:interpose :sep])
        "interpose calls its :interpose method")
(assert (= (partition 2 probe) [:partition 2])
        "partition calls its :partition method")
(assert (= (map-indexed (fn [i x] (+ i x)) probe) [:map-indexed 3])
        "map-indexed calls its :map-indexed method")
(assert (= (mapcat ten probe) [:mapcat 20]) "mapcat calls its :mapcat method")
(assert (= (take-while ten probe) [:take-while 20])
        "take-while calls its :take-while method")
(assert (= (drop-while ten probe) [:drop-while 20])
        "drop-while calls its :drop-while method")
(assert (= (sort-by ten probe) [:sort-by 20])
        "sort-by calls its :sort-by method")
(assert (= (sort-with (fn [a b] (- a b)) probe) [:sort-with -1])
        "sort-with calls its :sort-with method")

# The :Collection half of the table answers the same way.
(assert (= (fold (fn [acc x] (+ acc x)) 100 probe) [:fold 102])
        "fold calls its :fold method with (self f init)")
(assert (= (reduce (fn [acc x] (+ acc x)) 100 probe) [:fold 102])
        "reduce is fold, so it shares :fold")
(assert (= (count ten probe) [:count 20]) "count calls its :count method")
(assert (= (find-index ten probe) [:find-index 20])
        "find-index calls its :find-index method")
(assert (= (find ten probe) [:find 20]) "find calls its :find method")
(assert (= (any? ten probe) [:any? 20]) "any? calls its :any? method")
(assert (= (all? ten probe) [:all? 20]) "all? calls its :all? method")
(assert (= (update probe :k ten) [:update :k 50])
        "update calls its :update method with (self key f)")

# ============================================================================
# A builtin container family answers before the table is read
# ============================================================================
# `with-traits` on an array is still an array, so the array operator runs and
# the :map method is never consulted.

(begin
  (def traited-array
    (with-traits [1 2 3] @{:Sequence {:map (fn [self f] :hijacked)}}))
  (assert (not (nil? (get (traits traited-array) :Sequence)))
          "sanity: the table really is attached")
  (assert (= (map ten traited-array) [10 20 30])
          "a traited array maps as an array"))

# The trap, and why both values here are arrays: a table that declares a
# protocol REPLACES it for the access primitives, with no fall back to the
# default for a method it leaves out. A list carrying :Sequence loses `rest`,
# and one carrying :Collection loses `empty?`, so either would raise inside
# the walk before the assertion could read its answer. An array reaches its
# elements through `length` and `get`, which :Collection answers, so a
# :Sequence table leaves it whole.
(begin
  (def traited-reverse
    (with-traits [1 2 3] @{:Sequence {:reverse (fn [self] :hijacked)}}))
  (assert (= (reverse traited-reverse) [3 2 1])
          "a traited array reverses as an array"))

# ============================================================================
# A method beats the iterator
# ============================================================================
# The value below can answer either way. The per-operator method is the layer
# that wins, which is what lets a collection control what `map` means rather
# than only how its elements stream out.

(begin
  (def both
    (with-traits {:tag :both}
                 @{:Sequence {:map (fn [self f] :by-method)
                              :iter (fn [self]
                                      (fiber/new (fn []
                                        (yield 1)
                                        (yield 2)) |:yield|))}
                   :Collection {:empty (fn [self] ())
                                :conj (fn [self x] (pair x self))}}))
  (assert (= (map ten both) :by-method) "the method wins over :iter")
  (assert (= (fold (fn [acc x] (+ acc x)) 0 both) 3)
          "an operator with no method of its own still drives :iter"))

(println "trait-operator-methods: all tests passed")
