(elle/epoch 11)
## tests/elle/value-repr.lisp
## Value constructors, type names, truthiness, list ops

## ── type checking ──────────────────────────────────────────────────

(assert (nil? nil) "nil is nil")
(assert (not (boolean? nil)) "nil is not boolean")
(assert (not (integer? nil)) "nil is not integer")

(assert (boolean? true) "true is boolean")
(assert (boolean? false) "false is boolean")
(assert (= true true) "true eq")
(assert (= false false) "false eq")

## ── integer roundtrip ──────────────────────────────────────────────

(assert (= 0 0) "int 0")
(assert (= 1 1) "int 1")
(assert (= -1 -1) "int -1")
(assert (integer? 42) "42 is integer")

## ── float roundtrip ────────────────────────────────────────────────

(assert (float? 0.0) "0.0 is float")
(assert (float? 1.0) "1.0 is float")
(assert (float? (math/pi)) "pi is float")

## ── keyword ────────────────────────────────────────────────────────

(assert (keyword? :test) "keyword")
(assert (= :test :test) "keyword eq")

## ── string ─────────────────────────────────────────────────────────

(assert (string? "hello") "string")
(assert (string? "") "empty string")
(assert (= "hello" "hello") "string eq")
(assert (= "abcdefg" "abcdefg") "long string eq")

## ── pair/list ──────────────────────────────────────────────────────

(assert (list? (list 1 2 3)) "list? proper list")
(assert (list? ()) "list? nil")
(assert (= (first (pair 1 2)) 1) "pair first")
(assert (= (rest (pair 1 2)) 2) "pair rest")

## ── array ──────────────────────────────────────────────────────────

(assert (= (length @[1 2 3]) 3) "@array length")
(assert (= (get @[1 2 3] 0) 1) "@array get")

## ── struct ─────────────────────────────────────────────────────────

(let [t (@struct)]
  (assert (mutable? t) "@struct mutable"))

## ── box ────────────────────────────────────────────────────────────

(let [b (box 42)]
  (assert (box? b) "box? true")
  (assert (= (unbox b) 42) "unbox"))

## ── truthiness ─────────────────────────────────────────────────────

# Only nil and false are falsy
(assert (not nil) "nil is falsy")
(assert (not false) "false is falsy")
(assert true "true is truthy")
(assert 0 "0 is truthy")
(assert 0.0 "0.0 is truthy")
(assert "" "empty string is truthy")
(assert () "empty list is truthy")
(assert @[] "empty array is truthy")
(assert 1 "1 is truthy")
(assert -1 "-1 is truthy")
(assert "hello" "non-empty string is truthy")
(assert :test "keyword is truthy")

## ── pointer ────────────────────────────────────────────────────────

(assert (nil? (ptr/from-int 0)) "null pointer is nil")
(let [p (ptr/from-int 12345)]
  (assert (ptr? p) "non-null pointer is ptr"))

(println "value-repr: all tests passed")
