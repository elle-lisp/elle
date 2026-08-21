(elle/epoch 12)
## tests/elle/syntax-roundtrip.lisp
## Syntax ↔ Value roundtrips via quote/eval

## ── basic roundtrips ───────────────────────────────────────────────

(assert (nil? (eval 'nil)) "roundtrip nil")
(assert (= (eval '42) 42) "roundtrip int")
(assert (= (eval '1.5) 1.5) "roundtrip float")
(assert (= (eval 'true) true) "roundtrip bool true")
(assert (= (eval 'false) false) "roundtrip bool false")
(assert (= (eval '"hello") "hello") "roundtrip string")
(assert (= (eval ':bar) :bar) "roundtrip keyword")

## ── collection roundtrips ──────────────────────────────────────────

(assert (= (eval '()) ()) "roundtrip empty list")
(assert (= (eval '[1]) [1]) "roundtrip array")

(println "syntax-roundtrip: all tests passed")
