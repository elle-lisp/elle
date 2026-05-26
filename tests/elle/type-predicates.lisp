(elle/epoch 11)
## Type predicate tests
## Tests all type predicates moved from Rust to Elle.

## ── Single type tag predicates ──────────────────────────────────────

# nil?
(assert (nil? nil) "nil? on nil")
(assert (not (nil? 0)) "nil? on 0")
(assert (not (nil? false)) "nil? on false")
(assert (not (nil? "")) "nil? on empty string")
(assert (not (nil? (quote ()))) "nil? on empty list")

# integer?
(assert (integer? 42) "integer? on 42")
(assert (integer? -1) "integer? on -1")
(assert (integer? 0) "integer? on 0")
(assert (not (integer? 3.14)) "integer? on float")
(assert (not (integer? "42")) "integer? on string")
(assert (not (integer? nil)) "integer? on nil")

# float?
(assert (float? 3.14) "float? on 3.14")
(assert (float? 0.0) "float? on 0.0")
(assert (float? -1.5) "float? on -1.5")
(assert (not (float? 42)) "float? on int")
(assert (not (float? "3.14")) "float? on string")

# boolean?
(assert (boolean? true) "boolean? on true")
(assert (boolean? false) "boolean? on false")
(assert (not (boolean? nil)) "boolean? on nil")
(assert (not (boolean? 0)) "boolean? on 0")
(assert (not (boolean? 1)) "boolean? on 1")

# keyword?
(assert (keyword? :foo) "keyword? on :foo")
(assert (keyword? :bar) "keyword? on :bar")
(assert (not (keyword? "foo")) "keyword? on string")
(assert (not (keyword? 42)) "keyword? on int")

# native-fn?
(assert (native-fn? type-of) "native-fn? on type-of")
(assert (not (native-fn? 42)) "native-fn? on int")
(assert (not (native-fn? (fn [x] x))) "native-fn? on closure")

# closure?
(assert (closure? (fn [x] x)) "closure? on lambda")
(assert (not (closure? type-of)) "closure? on native-fn")
(assert (not (closure? 42)) "closure? on int")

# fiber?
(assert (not (fiber? 42)) "fiber? on int")
(assert (not (fiber? nil)) "fiber? on nil")

# box?
(assert (box? (box 1)) "box? on box")
(assert (not (box? 42)) "box? on int")
(assert (not (box? nil)) "box? on nil")

# parameter?
(assert (parameter? (make-parameter 0)) "parameter? on parameter")
(assert (not (parameter? 42)) "parameter? on int")
(assert (not (parameter? nil)) "parameter? on nil")

## ── Multiple type tag predicates ────────────────────────────────────

# number?
(assert (number? 42) "number? on int")
(assert (number? 3.14) "number? on float")
(assert (not (number? "42")) "number? on string")
(assert (not (number? nil)) "number? on nil")

# string?
(assert (string? "hello") "string? on string")
(assert (string? @"hello") "string? on mutable string")
(assert (string? "") "string? on empty string")
(assert (not (string? 42)) "string? on int")
(assert (not (string? :foo)) "string? on keyword")

# bytes?
(assert (bytes? (bytes 1 2 3)) "bytes? on bytes")
(assert (bytes? (@bytes 1 2 3)) "bytes? on mutable bytes")
(assert (not (bytes? "hello")) "bytes? on string")
(assert (not (bytes? 42)) "bytes? on int")

# array?
(assert (array? [1 2 3]) "array? on array")
(assert (array? @[1 2 3]) "array? on mutable array")
(assert (array? []) "array? on empty array")
(assert (not (array? 42)) "array? on int")
(assert (not (array? (list 1 2))) "array? on list")

# struct?
(assert (struct? {:a 1}) "struct? on struct")
(assert (struct? @{:a 1}) "struct? on mutable struct")
(assert (not (struct? 42)) "struct? on int")
(assert (not (struct? [1 2])) "struct? on array")

# set?
(assert (set? |1 2 3|) "set? on set")
(assert (set? (@set 1 2 3)) "set? on mutable set")
(assert (not (set? 42)) "set? on int")
(assert (not (set? [1 2 3])) "set? on array")

# fn?
(assert (fn? (fn [x] x)) "fn? on closure")
(assert (fn? type-of) "fn? on native-fn")
(assert (not (fn? 42)) "fn? on int")
(assert (not (fn? nil)) "fn? on nil")

# mutable?
(assert (mutable? @[1 2 3]) "mutable? on mutable array")
(assert (mutable? @{:a 1}) "mutable? on mutable struct")
(assert (mutable? @"hello") "mutable? on mutable string")
(assert (mutable? (@bytes 1 2)) "mutable? on mutable bytes")
(assert (mutable? (@set 1 2)) "mutable? on mutable set")
(assert (mutable? (box 1)) "mutable? on box")
(assert (not (mutable? [1 2 3])) "mutable? not on array")
(assert (not (mutable? {:a 1})) "mutable? not on struct")
(assert (not (mutable? "hello")) "mutable? not on string")
(assert (not (mutable? 42)) "mutable? not on int")

# immutable?
(assert (immutable? [1 2 3]) "immutable? on array")
(assert (immutable? {:a 1}) "immutable? on struct")
(assert (immutable? "hello") "immutable? on string")
(assert (immutable? 42) "immutable? on int")
(assert (immutable? nil) "immutable? on nil")
(assert (not (immutable? @[1 2 3])) "immutable? not on mutable array")
(assert (not (immutable? (box 1))) "immutable? not on box")

## ── Syntax-aware predicates ─────────────────────────────────────────

# pair?
(assert (pair? (list 1 2)) "pair? on list")
(assert (pair? (pair 1 2)) "pair? on pair")
(assert (not (pair? 42)) "pair? on int")
(assert (not (pair? nil)) "pair? on nil")
(assert (not (pair? (quote ()))) "pair? on empty list")

# list?
(assert (list? (list 1 2)) "list? on list")
(assert (list? (quote ())) "list? on empty list")
(assert (not (list? 42)) "list? on int")

# symbol?
(assert (symbol? (quote foo)) "symbol? on symbol")
(assert (not (symbol? 42)) "symbol? on int")
(assert (not (symbol? "foo")) "symbol? on string")
(assert (not (symbol? :foo)) "symbol? on keyword")

## ── Numeric predicates ──────────────────────────────────────────────

# zero?
(assert (zero? 0) "zero? on 0")
(assert (zero? 0.0) "zero? on 0.0")
(assert (not (zero? 1)) "zero? on 1")
(assert (not (zero? -1)) "zero? on -1")

# nonzero?
(assert (nonzero? 1) "nonzero? on 1")
(assert (nonzero? -1) "nonzero? on -1")
(assert (not (nonzero? 0)) "nonzero? on 0")
(assert (not (nonzero? 0.0)) "nonzero? on 0.0")

# nan? (depends on IEEE 754: NaN ≠ NaN)
(def nan-val (/ 0.0 0.0))
(assert (not (= nan-val nan-val)) "NaN ≠ NaN (IEEE 754)")
(assert (nan? nan-val) "nan? on NaN")
(assert (not (nan? 1.0)) "nan? on 1.0")
(assert (not (nan? 42)) "nan? on int")
(assert (not (nan? nil)) "nan? on nil")

# inf?
(assert (inf? (math/inf)) "inf? on +inf")
(assert (inf? (math/-inf)) "inf? on -inf")
(assert (not (inf? 1.0)) "inf? on 1.0")
(assert (not (inf? 42)) "inf? on int")
(assert (not (inf? nil)) "inf? on nil")

# pos?
(assert (pos? 1) "pos? on 1")
(assert (pos? 0.5) "pos? on 0.5")
(assert (not (pos? 0)) "pos? on 0")
(assert (not (pos? -1)) "pos? on -1")

# neg?
(assert (neg? -1) "neg? on -1")
(assert (neg? -0.5) "neg? on -0.5")
(assert (not (neg? 0)) "neg? on 0")
(assert (not (neg? 1)) "neg? on 1")

## ── Aliases ─────────────────────────────────────────────────────────

(assert (int? 42) "int? alias")
(assert (not (int? 3.14)) "int? alias negative")
(assert (bool? true) "bool? alias")
(assert (not (bool? 0)) "bool? alias negative")
(assert (native? type-of) "native? alias")
(assert (primitive? type-of) "primitive? alias")
(assert (positive? 1) "positive? alias")
(assert (negative? -1) "negative? alias")
(assert (infinite? (math/inf)) "infinite? alias")

## ── finite? (new predicate) ─────────────────────────────────────────

(assert (finite? 42) "finite? on int")
(assert (finite? 3.14) "finite? on float")
(assert (not (finite? (math/inf))) "finite? on +inf")
(assert (not (finite? (math/-inf))) "finite? on -inf")
(assert (not (finite? (/ 0.0 0.0))) "finite? on NaN")

## ── nil is NOT the empty list ────────────────────────────────────────

(assert (not (list? nil)) "list? on nil — nil ≠ ()")
(assert (not (pair? nil)) "pair? on nil")

(println "type-predicates: all passed")
