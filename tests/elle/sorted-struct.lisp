(elle/epoch 8)
# Tests for immutable struct operations
# Verifies sorted-array backing for immutable structs (Phase 0)
# and bump arena allocation (Phase 1).

# ── Construction and basic access ──────────────────────────────────────────

(assert (= (get {:a 1 :b 2 :c 3} :a) 1) "struct get :a")
(assert (= (get {:a 1 :b 2 :c 3} :b) 2) "struct get :b")
(assert (= (get {:a 1 :b 2 :c 3} :c) 3) "struct get :c")
(assert (= (get {:a 1} :z :default) :default) "struct get missing with default")
(assert (= (get {} :a) nil) "empty struct get returns nil")

# ── has? ───────────────────────────────────────────────────────────────────

(assert (has? {:a 1 :b 2} :a) "has? existing key")
(assert (has? {:a 1 :b 2} :b) "has? existing key b")
(assert (not (has? {:a 1 :b 2} :c)) "has? missing key")
(assert (not (has? {} :a)) "has? empty struct")

# ── keys and values ────────────────────────────────────────────────────────

(let* [s {:z 26 :a 1 :m 13}
       ks (keys s)
       vs (values s)]
  # Keys should be in sorted order (keyword ordering)
  (assert (= (length ks) 3) "keys length")
  (assert (= (length vs) 3) "values length"))

# ── put (immutable — returns new struct) ───────────────────────────────────

(let* [s {:a 1 :b 2}
       s2 (put s :c 3)]
  (assert (= (get s2 :a) 1) "put preserves :a")
  (assert (= (get s2 :b) 2) "put preserves :b")
  (assert (= (get s2 :c) 3) "put adds :c")
  (assert (= (get s :c) nil) "original unchanged after put"))

(let* [s {:a 1 :b 2}
       s2 (put s :a 99)]
  (assert (= (get s2 :a) 99) "put overwrites :a")
  (assert (= (get s :a) 1) "original unchanged after put overwrite"))

# ── del (immutable — returns new struct) ───────────────────────────────────

(let* [s {:a 1 :b 2 :c 3}
       s2 (del s :b)]
  (assert (= (get s2 :a) 1) "del preserves :a")
  (assert (= (get s2 :c) 3) "del preserves :c")
  (assert (not (has? s2 :b)) "del removes :b")
  (assert (has? s :b) "original unchanged after del"))

# ── pairs ──────────────────────────────────────────────────────────────────

(let [ps (pairs {:x 10 :y 20})]
  (assert (= (length ps) 2) "pairs length")
  # Each pair is [key value]
  (assert (= (length (first ps)) 2) "first pair is 2-element array"))

# ── freeze / thaw ─────────────────────────────────────────────────────────

(let* [m @{:a 1 :b 2}
       s (freeze m)]
  (assert (= (get s :a) 1) "freeze preserves :a")
  (assert (= (get s :b) 2) "freeze preserves :b")
  (assert (struct? s) "freeze produces immutable struct"))

(let* [s {:a 1 :b 2}
       m (thaw s)]
  (assert (= (get m :a) 1) "thaw preserves :a")
  (assert (= (type-of m) :@struct) "thaw produces @struct"))

# ── deep-freeze ────────────────────────────────────────────────────────────

(let* [inner @{:x 1}
       outer @{:nested inner}
       frozen (deep-freeze outer)]
  (assert (struct? frozen) "deep-freeze outer is struct")
  (assert (struct? (get frozen :nested)) "deep-freeze inner is struct"))

# ── equality ───────────────────────────────────────────────────────────────

(assert (= {:a 1 :b 2} {:b 2 :a 1}) "struct equality independent of insertion order")
(assert (= {:a 1} {:a 1}) "identical structs equal")
(assert (not (= {:a 1} {:a 2})) "different values not equal")
(assert (not (= {:a 1} {:a 1 :b 2})) "different sizes not equal")
(assert (= {} {}) "empty structs equal")

# ── struct/merge via put ───────────────────────────────────────────────────

(let* [a {:x 1 :y 2}
       m (put (put a :y 3) :z 4)]
  (assert (= (get m :x) 1) "merge preserves :x")
  (assert (= (get m :y) 3) "merge overwrites :y")
  (assert (= (get m :z) 4) "merge adds :z"))

# ── struct destructuring ──────────────────────────────────────────────────

(let [{:a a :b b :c c} {:a 10 :b 20 :c 30}]
  (assert (= a 10) "destructure :a")
  (assert (= b 20) "destructure :b")
  (assert (= c 30) "destructure :c"))

# StructRest via destructuring
(let [{:x x & rest} {:x 1 :y 2 :z 3}]
  (assert (= x 1) "destructure :x")
  (assert (struct? rest) "rest is struct")
  (assert (= (get rest :y) 2) "rest has :y")
  (assert (= (get rest :z) 3) "rest has :z")
  (assert (not (has? rest :x)) "rest excludes :x"))

# ── mixed key types ───────────────────────────────────────────────────────

(let [s (struct :a 1 "b" 2 42 3)]
  (assert (= (get s :a) 1) "keyword key")
  (assert (= (get s "b") 2) "string key")
  (assert (= (get s 42) 3) "int key"))

# ── hashing (structs can be set elements) ─────────────────────────────────

(let [s (set {:a 1} {:b 2})]
  (assert (has? s {:a 1}) "struct in set"))

# ── traits ─────────────────────────────────────────────────────────────────

(let* [tbl {:tag :test-type}
       obj (with-traits {:a 1} tbl)]
  (assert (struct? obj) "with-traits on struct preserves struct?")
  (assert (= (get obj :a) 1) "with-traits preserves data")
  (assert (= (get (traits obj) :tag) :test-type) "traits recoverable"))

(println "sorted-struct: all tests passed")
