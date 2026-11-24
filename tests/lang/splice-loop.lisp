(elle/epoch 18)
# audited: 2026-11-22
# A spliced call keeps every argument it has evaluated while a later argument runs a loop.
# docs/syntax.md
#
# The trap: an argument value that a loop in a later argument overwrote. The
# call then calls that earlier value, so `[s ;xs s]` with a struct `s` raised
# "struct call", and a call with a variadic callee returned the array
# constructor as its first element.

# Builds [lo..hi) with a loop written at the call site, inside the argument.
(defmacro range! [lo hi]
  `(let* [a @[]]
     (def @i ,lo)
     (while (< i ,hi)
       (push a i)
       (assign i (+ i 1)))
     (freeze a)))

(defn listed [& xs]
  xs)
(def s {:x 1})

(assert (= (list 5 3 4 6) (listed 5 ;(range! 3 5) 6))
        "an argument before a spliced loop survives it")
(assert (= [s 3 4 s] [s ;(range! 3 5) s])
        "an array literal keeps the element before a spliced loop")
(assert (= @[:a 3 4] @[:a ;(range! 3 5)]) "a mutable array literal keeps it too")
(assert (= [s 10 20 s] [s ;(map (fn [n] (* n 10)) [1 2]) s])
        "a spliced map keeps the element before it")
(assert (= (list 1 3 4) (listed ;(range! 1 2) ;(range! 3 5)))
        "a spliced argument survives the loop of a later spliced argument")
(assert (= (list 3 4 7) (listed ;(range! 3 5) 7))
        "a value after a spliced loop is unchanged")
(assert (= (list 1 3 4) ((fn [& xs] xs) 1 ;(range! 3 5)))
        "a lambda callee survives a spliced loop")

(defn tail-spliced [n]
  (listed n ;(range! 3 5)))
(assert (= (list 9 3 4) (tail-spliced 9))
        "a tail call keeps the argument before a spliced loop")

(println "splice-loop: ok")
