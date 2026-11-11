(elle/epoch 15)
# audited: 2026-11-11
# A mutable literal in a loop is a new value on every pass, and an immutable literal keeps its value after it leaves the loop.
# docs/syntax.md
#
# The counter-factual: a compiler that built every literal in a loop once, the
# mutable ones too, would hand each pass the buffer the pass before it wrote
# into. The buffers below would then read "xy", "xyy" and "xyyy".

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
    (var c 0)
    (while (< c cols)
      (let [b @""]
        (push b "z")
        (push out b))
      (assign c (+ c 1)))
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

(println "literal-fresh: ok")
