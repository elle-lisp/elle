(elle/epoch 15)
# audited: 2026-11-12
# A var that starts from a literal in a loop starts from that literal on every pass, however the pass reassigns it.
# docs/bindings.md
#
# The counter-factual: a compiler that built the literal once, before the loop,
# and let the var's first assign release it, would hand the next pass a freed
# value. The pass then reads whatever took that memory: a type error, or a
# string the shape of another pass's work.

# ── A single loop ───────────────────────────────────────────────────

(defn tagged [n]
  "n strings, each the literal \"x\" with its pass number appended once."
  (def out @[])
  (var i 0)
  (while (< i n)
    (var s "x")
    (assign s (string s i))
    (push out s)
    (assign i (+ i 1)))
  (freeze out))

(assert (= (tagged 3) ["x0" "x1" "x2"])
        "each pass starts its string from the literal")

(defn grown [n]
  "n arrays, each the literal [0] with the pass number appended once."
  (def out @[])
  (var i 0)
  (while (< i n)
    (var a [0])
    (assign a (append a [i]))
    (push out a)
    (assign i (+ i 1)))
  (freeze out))

(assert (= (grown 3) [[0 0] [0 1] [0 2]])
        "each pass starts its array from the literal")

# ── A nested loop ───────────────────────────────────────────────────

(defn joined [rows]
  "Each row of strings joined, from the literal \"\"."
  (def out @[])
  (each row in rows
    (var s "")
    (each w in row
      (assign s (string s w)))
    (push out s))
  (freeze out))

(assert (= (joined [["a" "b"] ["c" "d"] ["e"]]) ["ab" "cd" "e"])
        "each row starts its string from the literal")

(defn collected [n xs]
  "n copies of xs, each built from the literal [] by append."
  (def out @[])
  (repeat n
    (var acc [])
    (each x in xs
      (assign acc (append acc [x])))
    (push out acc))
  (freeze out))

(assert (= (collected 3 [1 2]) [[1 2] [1 2] [1 2]])
        "each pass starts its array from the literal")

# The shape that found the defect: the run restarts from the literal inside
# the inner loop as well as on each pass of the outer one.
(defn pairs-twice [xs]
  "xs in runs of two, taken twice; each run starts from the literal []."
  (var out [])
  (each _ in [1 2]
    (var run [])
    (each x in xs
      (when (> (length run) 1)
        (assign out (append out [run]))
        (assign run []))
      (assign run (append run [x]))))
  out)

(assert (= (pairs-twice [1 2 3 4 5]) [[1 2] [3 4] [1 2] [3 4]])
        "every run starts from the empty array")

(println "literal-var: ok")
