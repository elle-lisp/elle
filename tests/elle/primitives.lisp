(elle/epoch 10)
## ── not= ──────────────────────────────────────────────────────────────

(assert (not= 1 2) "not= different ints")
(assert (not (not= 1 1)) "not= same int")
(assert (not (not= 1 1.0)) "not= numeric coercion")
(assert (not= "a" "b") "not= different strings")
(assert (not (not= :foo :foo)) "not= same keyword")

## ── hash ──────────────────────────────────────────────────────────────

(assert (= (hash :foo) (hash :foo)) "hash deterministic")
(assert (not= (hash :foo) (hash :bar)) "hash different values differ")
(assert (integer? (hash "hello")) "hash returns integer")
(assert (= (hash 42) (hash 42)) "hash integer deterministic")
(assert (= (hash [1 2]) (hash [1 2])) "hash structural")

## ── deep-freeze ───────────────────────────────────────────────────────

(let* [m @[@[1 2] @{:a @[3]}]
       f (deep-freeze m)]
  (assert (immutable? f) "deep-freeze outer")
  (assert (immutable? (get f 0)) "deep-freeze nested array")
  (assert (immutable? (get f 1)) "deep-freeze nested struct")
  (assert (immutable? (get (get f 1) :a)) "deep-freeze deeply nested"))

(assert (= (deep-freeze 42) 42) "deep-freeze atom passthrough")
(assert (= (deep-freeze nil) nil) "deep-freeze nil passthrough")

(let* [lst (list @[1] @[2])
       f (deep-freeze lst)]
  (assert (immutable? (first f)) "deep-freeze list element")
  (assert (immutable? (first (rest f))) "deep-freeze list second element"))

## ── immutable? ────────────────────────────────────────────────────────

(assert (immutable? [1 2]) "immutable? array")
(assert (immutable? {:a 1}) "immutable? struct")
(assert (immutable? "hello") "immutable? string")
(assert (immutable? 42) "immutable? integer")
(assert (not (immutable? @[1])) "immutable? @array")
(assert (not (immutable? @{:a 1})) "immutable? @struct")

## ── nan? pos? neg? inf? ───────────────────────────────────────────────

(assert (nan? (asin 2.0)) "nan? true via asin domain error")
(assert (not (nan? 1.0)) "nan? false for normal float")
(assert (not (nan? 42)) "nan? false for integer")

(assert (pos? 1) "pos? positive int")
(assert (pos? 0.5) "pos? positive float")
(assert (not (pos? 0)) "pos? zero")
(assert (not (pos? -1)) "pos? negative")

(assert (neg? -1) "neg? negative int")
(assert (neg? -0.5) "neg? negative float")
(assert (not (neg? 0)) "neg? zero")
(assert (not (neg? 1)) "neg? positive")

(assert (not (inf? 1.0)) "inf? false for finite")
(assert (not (inf? 42)) "inf? false for integer")

## ── string/repeat ─────────────────────────────────────────────────────

(assert (= (string/repeat "ab" 3) "ababab") "string/repeat basic")
(assert (= (string/repeat "-" 0) "") "string/repeat zero")
(assert (= (string/repeat "x" 1) "x") "string/repeat one")
(assert (= (string/repeat "" 5) "") "string/repeat empty string")

## ── math functions ────────────────────────────────────────────────────

(assert (< (abs (- (asin 1.0) (/ (pi) 2))) 0.0001) "asin pi/2")
(assert (< (abs (acos 1.0)) 0.0001) "acos 0")
(assert (< (abs (- (atan 1.0) (/ (pi) 4))) 0.0001) "atan pi/4")
(assert (< (abs (- (atan2 1.0 1.0) (/ (pi) 4))) 0.0001) "atan2")
(assert (< (abs (sinh 0.0)) 0.0001) "sinh 0")
(assert (< (abs (- (cosh 0.0) 1.0)) 0.0001) "cosh 0")
(assert (< (abs (tanh 0.0)) 0.0001) "tanh 0")
(assert (< (abs (- (log2 8.0) 3.0)) 0.0001) "log2")
(assert (< (abs (- (log10 1000.0) 3.0)) 0.0001) "log10")
(assert (= (trunc 3.7) 3.0) "trunc positive")
(assert (= (trunc -3.7) -3.0) "trunc negative")
(assert (< (abs (- (cbrt 27.0) 3.0)) 0.0001) "cbrt")
(assert (< (abs (- (exp2 3.0) 8.0)) 0.0001) "exp2")

## ── repeat macro ──────────────────────────────────────────────────────

(def @count 0)
(repeat 5 (assign count (+ count 1)))
(assert (= count 5) "repeat runs N times")

(def @count2 0)
(repeat 0 (assign count2 (+ count2 1)))
(assert (= count2 0) "repeat 0 runs nothing")

## ── from-pairs ────────────────────────────────────────────────────────

(assert (= (from-pairs [[:a 1] [:b 2]]) {:a 1 :b 2}) "from-pairs arrays")
(assert (= (from-pairs (list (list :x 10))) {:x 10}) "from-pairs lists")
(assert (= (from-pairs []) {}) "from-pairs empty")
(assert (= (from-pairs (pairs {:a 1 :b 2})) {:a 1 :b 2}) "from-pairs roundtrip")

## ── sum / product ─────────────────────────────────────────────────────

(assert (= (sum [1 2 3 4]) 10) "sum")
(assert (= (sum []) 0) "sum empty")
(assert (= (product [1 2 3 4]) 24) "product")
(assert (= (product []) 1) "product empty")

## ── update ────────────────────────────────────────────────────────────

(assert (= (update {:count 5} :count inc) {:count 6}) "update struct")
(assert (= (update [10 20 30] 1 inc) [10 21 30]) "update array")
(assert (= (update @{:x 2} :x (fn [v] (* v 3))) @{:x 6}) "update @struct")

(def [ok? err] (protect (update {:a 1} :b inc)))
(assert (not ok?) "update missing key errors")
(assert (= err:error :key-error) "update error is :key-error")

(def [ok2? err2] (protect (update [1 2] 5 inc)))
(assert (not ok2?) "update out-of-bounds errors")
(assert (= err2:error :key-error) "update oob is :key-error")

## ── cross-mutability equality ─────────────────────────────────────────

(assert (= [1 2 3] @[1 2 3]) "array = @array")
(assert (= @[1 2 3] [1 2 3]) "@array = array")
(assert (= {:a 1} @{:a 1}) "struct = @struct")
(assert (= @{:a 1} {:a 1}) "@struct = struct")
(assert (= "hello" (thaw "hello")) "string = @string")
(assert (= (bytes 1 2) (@bytes 1 2)) "bytes = @bytes")
(assert (= |1 2 3| @|1 2 3|) "set = @set")
(assert (not= [1 2] @[1 3]) "cross-mut different contents")

## ── nonzero? ──────────────────────────────────────────────────────────

(assert (nonzero? 1) "nonzero? positive int")
(assert (nonzero? -1) "nonzero? negative int")
(assert (nonzero? 0.5) "nonzero? float")
(assert (not (nonzero? 0)) "nonzero? zero int")
(assert (not (nonzero? 0.0)) "nonzero? zero float")

## ── get-in ────────────────────────────────────────────────────────────

(assert (= (get-in {:a {:b 1}} [:a :b]) 1) "get-in nested struct")
(assert (= (get-in {:a [10 20 30]} [:a 1]) 20) "get-in struct then array")
(assert (= (get-in [[1 2] [3 4]] [1 0]) 3) "get-in nested arrays")

## ── put-in ────────────────────────────────────────────────────────────

(assert (= (put-in {:a {:b 1}} [:a :b] 2) {:a {:b 2}}) "put-in nested struct")
(assert (= (put-in {:a [10 20]} [:a 1] 99) {:a [10 99]})
        "put-in struct then array")
(assert (= (put-in [0 [1 2]] [1 0] 9) [0 [9 2]]) "put-in nested arrays")

## ── update-in ─────────────────────────────────────────────────────────

(assert (= (update-in {:a {:b 5}} [:a :b] inc) {:a {:b 6}})
        "update-in nested struct")
(assert (= (update-in [10 [20 30]] [1 0] inc) [10 [21 30]])
        "update-in nested arrays")

(def [ok? err] (protect (update-in {:a {:b 1}} [:a :x] inc)))
(assert (not ok?) "update-in missing key errors")

## ── nonzero? ──────────────────────────────────────────────────────────

(assert (nonzero? 1) "nonzero? positive int")
(assert (nonzero? -1) "nonzero? negative int")
(assert (not (nonzero? 0)) "nonzero? zero int")
(assert (not (nonzero? 0.0)) "nonzero? zero float")

## ── ->array / ->list ──────────────────────────────────────────────────

(assert (= (->array (list 1 2 3)) [1 2 3]) "->array from list")
(assert (= (->array @[1 2 3]) [1 2 3]) "->array from @array")
(assert (= (->array [1 2 3]) [1 2 3]) "->array from array passthrough")
(assert (= (->array "abc") ["a" "b" "c"]) "->array from string")
(assert (= (->array (bytes 1 2 3)) [1 2 3]) "->array from bytes")
(assert (= (->array ()) []) "->array from empty list")

(assert (= (->list [1 2 3]) (list 1 2 3)) "->list from array")
(assert (= (->list @[1 2 3]) (list 1 2 3)) "->list from @array")
(assert (= (->list (list 1 2)) (list 1 2)) "->list from list passthrough")
(assert (= (->list "abc") (list "a" "b" "c")) "->list from string")
(assert (= (->list []) ()) "->list from empty array")

(println "all primitives tests passed")
