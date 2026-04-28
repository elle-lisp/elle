(elle/epoch 9)
## Callable collections test suite
##
## Tests for calling structs, arrays, and sets as functions.

# ── Struct as function ────────────────────────────────────────────────
(let [m {:a 1 :b 2 :c 3}]
  (assert (= (m :a) 1) "struct lookup :a")
  (assert (= (m :b) 2) "struct lookup :b")
  (assert (= (m :c) 3) "struct lookup :c")
  (assert (= (m :z) nil) "struct missing key returns nil")
  (assert (= (m :z :default) :default) "struct missing key with fallback"))

# ── @Struct as function ───────────────────────────────────────────────
(let [m @{:x 10 :y 20}]
  (assert (= (m :x) 10) "@struct lookup :x")
  (assert (= (m :missing) nil) "@struct missing returns nil")
  (assert (= (m :missing 99) 99) "@struct missing with fallback"))

# ── Array as function ─────────────────────────────────────────────────
(let [a [10 20 30]]
  (assert (= (a 0) 10) "array index 0")
  (assert (= (a 1) 20) "array index 1")
  (assert (= (a 2) 30) "array index 2")
  (assert (= (a 5) nil) "array out-of-bounds returns nil")
  (assert (= (a -1) 30) "array negative index wraps")
  (assert (= (a 99 :oob) :oob) "array out-of-bounds with fallback"))

# ── @Array as function ────────────────────────────────────────────────
(let [a @[100 200 300]]
  (assert (= (a 0) 100) "@array index 0")
  (assert (= (a 5) nil) "@array out-of-bounds returns nil")
  (assert (= (a 5 :nope) :nope) "@array out-of-bounds with fallback"))

# ── Set as predicate ──────────────────────────────────────────────────
(let [s |1 2 3|]
  (assert (= (s 1) true) "set contains 1")
  (assert (= (s 2) true) "set contains 2")
  (assert (= (s 4) false) "set does not contain 4"))

# ── @Set as predicate ─────────────────────────────────────────────────
(let [s @|:a :b :c|]
  (assert (= (s :a) true) "@set contains :a")
  (assert (= (s :z) false) "@set does not contain :z"))

# ── Higher-order: struct as key extractor ─────────────────────────────
(let [m {:a 1 :b 2 :c 3}]
  (assert (= (map m [:a :b :c]) [1 2 3]) "map struct over keys"))

# ── Higher-order: set as filter predicate ─────────────────────────────
(let [allowed |1 3 5|]
  (assert (= (filter allowed [1 2 3 4 5]) [1 3 5]) "filter with set predicate"))

# ── Higher-order: array as lookup ─────────────────────────────────────
(let [a [:zero :one :two :three]]
  (assert (= (map a [0 2 3]) [:zero :two :three]) "map array over indices"))

# ── callable? predicate ───────────────────────────────────────────────
(assert (= (callable? +) true) "callable?: native fn")
(assert (= (callable? (fn [x] x)) true) "callable?: closure")
(assert (= (callable? {:a 1}) true) "callable?: struct")
(assert (= (callable? @{:a 1}) true) "callable?: @struct")
(assert (= (callable? [1 2]) true) "callable?: array")
(assert (= (callable? @[1 2]) true) "callable?: @array")
(assert (= (callable? |1 2|) true) "callable?: set")
(assert (= (callable? @|1 2|) true) "callable?: @set")
(assert (= (callable? 42) false) "callable?: integer")
(assert (= (callable? :keyword) false) "callable?: keyword")
(assert (= (callable? nil) false) "callable?: nil")

# ── fn? unchanged ─────────────────────────────────────────────────────
(assert (= (fn? {:a 1}) false) "fn?: struct is not fn")
(assert (= (fn? [1 2]) false) "fn?: array is not fn")
(assert (= (fn? |1 2|) false) "fn?: set is not fn")
(assert (= (fn? +) true) "fn?: native fn is fn")
(assert (= (fn? (fn [x] x)) true) "fn?: closure is fn")

# ── String as function (grapheme index) ────────────────────────────
(assert (= ("food" 0) "f") "string index 0")
(assert (= ("food" 1) "o") "string index 1")
(assert (= ("food" 3) "d") "string index 3")
(assert (= ("food" 10) nil) "string out-of-bounds returns nil")
(assert (= ("food" 10 :oob) :oob) "string out-of-bounds with fallback")
(assert (= ("food" -1) "d") "string negative index wraps")

# ── @String as function ───────────────────────────────────────────
(let [s (thaw "hello")]
  (assert (= (s 0) "h") "@string index 0")
  (assert (= (s 4) "o") "@string index 4")
  (assert (= (s 99) nil) "@string out-of-bounds returns nil"))

# ── Bytes as function ─────────────────────────────────────────────
(let [b (bytes 97 98 99)]
  (assert (= (b 0) 97) "bytes index 0")
  (assert (= (b 1) 98) "bytes index 1")
  (assert (= (b 2) 99) "bytes index 2")
  (assert (= (b 5) nil) "bytes out-of-bounds returns nil")
  (assert (= (b 5 -1) -1) "bytes out-of-bounds with fallback"))

# ── @Bytes as function ────────────────────────────────────────────
(let [b (@bytes 10 20 30)]
  (assert (= (b 0) 10) "@bytes index 0")
  (assert (= (b 99) nil) "@bytes out-of-bounds returns nil"))

# ── callable? for strings and bytes ───────────────────────────────
(assert (= (callable? "hello") true) "callable?: string")
(assert (= (callable? (thaw "hello")) true) "callable?: @string")
(assert (= (callable? (bytes 1 2)) true) "callable?: bytes")
(assert (= (callable? (@bytes 1 2)) true) "callable?: @bytes")

# ── Error cases ───────────────────────────────────────────────────────
(assert (protect
          (({:a 1}))
          :error) "struct call with 0 args is arity error")
(assert (protect
          (([1 2] "x"))
          :error) "array call with string index is type error")

# ── Tail position ─────────────────────────────────────────────────────
(defn lookup [m k]
  (m k))
(assert (= (lookup {:x 42} :x) 42) "callable struct in tail position")

(println "all callable collection tests passed")
