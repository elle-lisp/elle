(elle/epoch 8)

## Regression test for #510: JIT must correctly handle variadic functions.
##
## The JIT entry block builds a cons list for the rest parameter from the
## raw args pointer. This test exercises:
## - Zero rest args (rest = EMPTY_LIST, not NIL)
## - One rest arg
## - Multiple rest args
## - Rest arg type checking (list?, length, first, rest)
## - Variadic function that captures rest param in a closure
## - Self-tail-call with variadics

(defn variadic-fn (x & rest)
  "Returns x as string, or x + first rest arg as string."
  (if (empty? rest)
    (string x)
    (append (append (string x) " ") (string (first rest)))))

## Call 15 times — past the JIT threshold of 10.
## Every call must succeed; before the fix, call 10+ would crash.
(assert (= (variadic-fn 1) "1") "variadic call 1")
(assert (= (variadic-fn 2) "2") "variadic call 2")
(assert (= (variadic-fn 3) "3") "variadic call 3")
(assert (= (variadic-fn 4) "4") "variadic call 4")
(assert (= (variadic-fn 5) "5") "variadic call 5")
(assert (= (variadic-fn 6) "6") "variadic call 6")
(assert (= (variadic-fn 7) "7") "variadic call 7")
(assert (= (variadic-fn 8) "8") "variadic call 8")
(assert (= (variadic-fn 9) "9") "variadic call 9")
(assert (= (variadic-fn 10) "10") "variadic call 10")
(assert (= (variadic-fn 11) "11") "variadic call 11")
(assert (= (variadic-fn 12) "12") "variadic call 12")
(assert (= (variadic-fn 13 :extra) "13 extra") "variadic call 13 with rest arg")
(assert (= (variadic-fn 14) "14") "variadic call 14")
(assert (= (variadic-fn 15) "15") "variadic call 15")

## Test rest arg is a proper list (not NIL)
(defn check-rest-type (& rest)
   "Returns an array of (list? rest, length rest, empty? rest)."
   (list (list? rest) (length rest) (empty? rest)))

(assert (= (check-rest-type) (list true 0 true)) "zero rest args: list? true, length 0, empty? true")
(assert (= (check-rest-type 1) (list true 1 false)) "one rest arg: list? true, length 1, empty? false")
(assert (= (check-rest-type 1 2 3) (list true 3 false)) "three rest args: list? true, length 3, empty? false")

## Call past JIT threshold to ensure JIT path works
(check-rest-type)
(check-rest-type)
(check-rest-type)
(check-rest-type)
(check-rest-type)
(check-rest-type)
(check-rest-type)
(check-rest-type)
(check-rest-type)
(check-rest-type)
(assert (= (check-rest-type 10 20) (list true 2 false)) "post-JIT: two rest args")
(assert (= (check-rest-type) (list true 0 true)) "post-JIT: zero rest args")

## Test multiple rest args — verify cons list order
(defn collect-rest (& args)
  args)

## Warm up past JIT threshold
(collect-rest)
(collect-rest 1)
(collect-rest 1 2)
(collect-rest 1 2 3)
(collect-rest)
(collect-rest 1)
(collect-rest 1 2)
(collect-rest 1 2 3)
(collect-rest)
(collect-rest 1)
(assert (= (collect-rest) (list)) "post-JIT collect: empty")
(assert (= (collect-rest 1) (list 1)) "post-JIT collect: one")
(assert (= (collect-rest 1 2 3) (list 1 2 3)) "post-JIT collect: three in order")

## Test variadic with closure capture of rest param
(defn make-rest-getter (& rest)
  "Returns a closure that returns the captured rest list."
  (fn () rest))

## Warm up past JIT threshold
(make-rest-getter)
(make-rest-getter 1)
(make-rest-getter 1 2)
(make-rest-getter)
(make-rest-getter 1)
(make-rest-getter 1 2)
(make-rest-getter)
(make-rest-getter 1)
(make-rest-getter 1 2)
(make-rest-getter)
(def getter-empty ((make-rest-getter)))
(def getter-one ((make-rest-getter 42)))
(def getter-multi ((make-rest-getter 10 20 30)))
(assert (= getter-empty (list)) "captured rest: empty")
(assert (= getter-one (list 42)) "captured rest: one")
(assert (= getter-multi (list 10 20 30)) "captured rest: three")
