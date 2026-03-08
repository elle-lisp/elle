(import-file "tests/elle/assert.lisp")

## Regression test for #510: JIT must not break variadic functions.
##
## The JIT entry block loads only fixed_params() arguments. For variadic
## functions (AtLeast arity), the rest-parameter slot was never initialized,
## becoming NIL instead of EMPTY_LIST. After 10 calls (JIT threshold), the
## function would crash on (empty? opts) because NIL is not a collection.
##
## The fix rejects variadic functions from JIT compilation so they always
## run through the interpreter, which handles rest-arg collection correctly.

(defn variadic-fn (x & rest)
  "Returns x as string, or x + first rest arg as string."
  (if (empty? rest)
    (string x)
    (append (append (string x) " ") (string (first rest)))))

## Call 15 times — past the JIT threshold of 10.
## Every call must succeed; before the fix, call 10+ would crash.
(assert-eq (variadic-fn 1) "1" "variadic call 1")
(assert-eq (variadic-fn 2) "2" "variadic call 2")
(assert-eq (variadic-fn 3) "3" "variadic call 3")
(assert-eq (variadic-fn 4) "4" "variadic call 4")
(assert-eq (variadic-fn 5) "5" "variadic call 5")
(assert-eq (variadic-fn 6) "6" "variadic call 6")
(assert-eq (variadic-fn 7) "7" "variadic call 7")
(assert-eq (variadic-fn 8) "8" "variadic call 8")
(assert-eq (variadic-fn 9) "9" "variadic call 9")
(assert-eq (variadic-fn 10) "10" "variadic call 10")
(assert-eq (variadic-fn 11) "11" "variadic call 11")
(assert-eq (variadic-fn 12) "12" "variadic call 12")
(assert-eq (variadic-fn 13 :extra) "13 :extra" "variadic call 13 with rest arg")
(assert-eq (variadic-fn 14) "14" "variadic call 14")
(assert-eq (variadic-fn 15) "15" "variadic call 15")
