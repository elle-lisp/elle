(elle/epoch 10)
# Tests for expanded TableKey: immutable compound types as struct keys

# ── Cons cell / list as struct key ──────────────────────────────────────

(let [m (struct (list 1 2 3) :yes)]
  (assert (= (get m (list 1 2 3)) :yes) "list as struct key"))

(let [m (struct (list :a (list 1 2)) :nested)]
  (assert (= (get m (list :a (list 1 2))) :nested) "nested list as struct key"))

# ── Empty list as struct key ────────────────────────────────────────────

(let [m (struct () :empty)]
  (assert (= (get m ()) :empty) "empty list as struct key"))

# ── Immutable bytes as struct key ───────────────────────────────────────

(let [b (bytes 1 2 3)
      m (struct b :found)]
  (assert (= (get m b) :found) "bytes as struct key"))

# ── Immutable set as struct key ─────────────────────────────────────────

(let [m (struct |1 2 3| :found)]
  (assert (= (get m |1 2 3|) :found) "set as struct key"))

# ── Immutable struct as struct key ──────────────────────────────────────

(let [m (struct {:a 1} :nested)]
  (assert (= (get m {:a 1}) :nested) "struct as struct key"))

(let [m (struct {:a 1 :b 2} :two)]
  (assert (= (get m {:b 2 :a 1}) :two) "struct key independent of order"))

# ── Negative: float as key → error ─────────────────────────────────────

(let [[ok? err] (protect ((fn [] (struct 1.5 :v))))]
  (assert (not ok?) "float as struct key errors")
  (assert (= (get err :error) :type-error) "float as struct key error kind"))

# ── Negative: mutable box as key → error ───────────────────────────────

(let [[ok? err] (protect ((fn [] (struct (box 1) :v))))]
  (assert (not ok?) "box as struct key errors")
  (assert (= (get err :error) :type-error) "box as struct key error kind"))

# ── Negative: @array as key → error ────────────────────────────────────

(let [[ok? err] (protect ((fn [] (struct @[1 2] :v))))]
  (assert (not ok?) "@array as struct key errors")
  (assert (= (get err :error) :type-error) "@array as struct key error kind"))

# ── Negative: struct with mutable sub-value as key → error ──────────────

(let [[ok? err] (protect ((fn []
                            (def s {:a (box 1)})
                            (struct s :v))))]
  (assert (not ok?) "struct with box value as struct key errors")
  (assert (= (get err :error) :type-error) "struct with box value error kind"))

# ── Negative: list containing float → error ────────────────────────────

(let [[ok? err] (protect ((fn [] (struct (list 1.5) :v))))]
  (assert (not ok?) "list containing float as struct key errors")
  (assert (= (get err :error) :type-error) "list containing float error kind"))

# ── Negative: set containing float → error ─────────────────────────────

(let [[ok? err] (protect ((fn [] (struct (set 1.5) :v))))]
  (assert (not ok?) "set containing float as struct key errors")
  (assert (= (get err :error) :type-error) "set containing float error kind"))

# ── Equality semantics: struct keys are compared structurally ───────────

(let [m (struct {:x 1} :val)]
  (assert (= (get m {:x 1}) :val) "struct keys use structural equality"))

# ── put and del with compound keys ─────────────────────────────────────

(let* [s (struct (list 1 2) :a)
       s2 (put s (list 3 4) :b)]
  (assert (= (get s2 (list 1 2)) :a) "put preserves list key")
  (assert (= (get s2 (list 3 4)) :b) "put adds list key"))

(let* [s (struct (list 1 2) :a (list 3 4) :b)
       s2 (del s (list 1 2))]
  (assert (not (has? s2 (list 1 2))) "del removes list key")
  (assert (= (get s2 (list 3 4)) :b) "del preserves other list key"))

# ── has? with compound keys ────────────────────────────────────────────

(assert (has? (struct (list 1 2) :v) (list 1 2)) "has? with list key")
(assert (not (has? (struct (list 1 2) :v) (list 3 4)))
        "has? with missing list key")

(println "table-key-expand: all tests passed")
