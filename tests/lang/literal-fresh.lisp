(elle/epoch 16)
# audited: 2026-11-17
# A mutable literal in a loop or a function is new on every pass and call, and an immutable one keeps its value after it leaves.
# docs/syntax.md
#
# The counter-factual: a compiler that built every literal in a loop or a
# function once, the mutable ones too, would hand each pass the buffer the
# pass before it wrote into. The buffers below would then read "xy", "xyy"
# and "xyyy", and the second call's buffer would read "xy".

(defn fill [n make]
  "An array of the n values (make) answers, each written once before it is kept."
  (def out @[])
  (var i 0)
  (while (< i n)
    (push out (make))
    (assign i (+ i 1)))
  (freeze out))

# ── A mutable literal is new on every pass ──────────────────────────

(defn buffers [n]
  "n buffers, each the literal @\"x\" with \"y\" pushed onto it."
  (def out @[])
  (var i 0)
  (while (< i n)
    (let [b @"x"]
      (push b "y")
      (push out b))
    (assign i (+ i 1)))
  (freeze out))

(each b in (buffers 3)
  (assert (= (freeze b) "xy") "each pass builds a new @string"))

(defn arrays [n]
  "n arrays, each the literal @[1] with 2 pushed onto it."
  (def out @[])
  (var i 0)
  (while (< i n)
    (let [a @[1]]
      (push a 2)
      (push out a))
    (assign i (+ i 1)))
  (freeze out))

(each a in (arrays 3)
  (assert (= (freeze a) [1 2]) "each pass builds a new @array"))

(defn tables [n]
  "n tables, each the literal @{:k \"v\"} with :n put into it."
  (def out @[])
  (var i 0)
  (while (< i n)
    (let [t @{:k "v"}]
      (put t :n i)
      (push out t))
    (assign i (+ i 1)))
  (freeze out))

(def ts (tables 3))
(assert (= (get (get ts 0) :n) 0) "the first pass's table keeps its own write")
(assert (= (get (get ts 2) :n) 2) "the last pass's table keeps its own write")

(defn sets [n]
  "n sets, each the literal @|\"a\"| with i added to it."
  (def out @[])
  (var i 0)
  (while (< i n)
    (let [s @|"a"|]
      (add s i)
      (push out s))
    (assign i (+ i 1)))
  (freeze out))

(def ss (sets 3))
(assert (= (length (get ss 2)) 2) "each pass builds a new @set")
(assert (not (has? (get ss 2) 0)) "a later set holds no earlier pass's element")

# A nested loop runs its inner body on every pass of the outer one, and the
# literal is new on every inner pass.
(defn grid [rows cols]
  (def out @[])
  (var r 0)
  (while (< r rows)
    (let [@c 0]
      (while (< c cols)
        (let [b @""]
          (push b "z")
          (push out b))
        (assign c (+ c 1))))
    (assign r (+ r 1)))
  (freeze out))

(def cells (grid 2 3))
(assert (= (length cells) 6) "one buffer per inner pass")
(each b in cells
  (assert (= (freeze b) "z") "each inner pass builds a new @string"))

# ── An immutable literal keeps its value after the loop ─────────────

(def tags (fill 4 (fn [] "tag")))
(assert (= tags ["tag" "tag" "tag" "tag"]) "a string literal outlives its loop")

(defn pushed [n]
  "n copies of each kind of immutable literal, pushed in one loop."
  (def out @[])
  (var i 0)
  (while (< i n)
    (push out ["tag" '(1 "q") [1 "a"] {:k "v"} |"s"| b[1 2]])
    (assign i (+ i 1)))
  (freeze out))

(each row in (pushed 3)
  (assert (= row ["tag" '(1 "q") [1 "a"] {:k "v"} |"s"| b[1 2]])
          "every immutable literal keeps its value after the loop"))

# A literal compared on every pass answers as it would outside the loop.
(defn count-bars [gs]
  (var n 0)
  (each g in gs
    (when (= g "|") (assign n (+ n 1)))
    (when (has? |"-" "+"| g) (assign n (+ n 10))))
  n)

(assert (= (count-bars (->array "a|b-c|+")) 22)
        "the literals compare by contents")

# ── A literal in a function ─────────────────────────────────────────

(defn buffer []
  "A new buffer, the literal @\"x\"."
  @"x")

(def b1 (buffer))
(def b2 (buffer))
(push b1 "y")
(assert (= (freeze b1) "xy") "the first call's buffer takes its write")
(assert (= (freeze b2) "x") "the second call's buffer is a new one")

(defn table []
  "A new table, the literal @{:k \"v\"}."
  @{:k "v"})

(def t1 (table))
(put t1 :n 1)
(assert (nil? (get (table) :n)) "each call builds a new @struct")

(defn row []
  "One of each kind of immutable literal."
  ["tag" '(1 "q") [1 "a"] {:k "v"} |"s"| b[1 2]])

(def rows [(row) (row) (row)])
(each r in rows
  (assert (= r ["tag" '(1 "q") [1 "a"] {:k "v"} |"s"| b[1 2]])
          "every immutable literal keeps its value after the call"))

# A collection literal a function returns is a call in tail position. Built
# once at the start of the unit, it is no tail call, and every constant built
# before it keeps its value.
(defn greeter []
  "A function that answers the string literal \"hello\"."
  (fn [] "hello"))
(defn pair []
  "The array literal [\"x\" \"y\"]."
  ["x" "y"])

(def p (pair))
(assert (= ((greeter)) "hello")
        "a constant built before a returned literal keeps its value")
(assert (= p ["x" "y"]) "and the returned literal holds its items")

(defn bar? [g]
  (= g "|"))
(defn mark? [g]
  (has? |"-" "+"| g))
(assert (= (length (filter bar? (->array "a|b-c|+"))) 2)
        "a string literal in a predicate compares by contents")
(assert (= (length (filter mark? (->array "a|b-c|+"))) 2)
        "and so does a set literal")

(println "literal-fresh: ok")
