(elle/epoch 18)
# audited: 2026-11-20
# std/check's bit sequences: the width of a bound, writing and reading a choice, simplicity, and the record a source keeps.
# docs/stdlib/check/overview.md
#
# The counter-factual: a width one bit short reads 512 of a bound of 1001 as
# 256, and every draw still answers inside its bound. So each test names the
# bits of a choice, not only the choice.

(include-file "check/parts.lisp")

(defn reader [xs]
  "A function that answers the bits of XS in order, then 0."
  (var i 0)
  (fn []
    (let [b (if (< i (length xs)) (get xs i) 0)]
      (assign i (+ i 1))
      b)))

# ── width, write and read ─────────────────────────────────────────────
(assert (= [0 1 2 2 3 10 62] (map bits:width [1 2 3 4 5 1001 (bit/shl 1 62)]))
        "a bound's width is the bits that write the bound less one")
(assert (= [0 1 0 1] (bits:encode 5 4))
        "encode writes the most significant first")
(assert (= [] (bits:encode 0 0)) "in as many bits as it is given")
(let [out @[1]]
  (assert (= @[1 0 1 0] (bits:write out 2 3)) "write pushes onto an array")
  (assert (= @[1 0 1 0] out) "the array it was given"))
(assert (= 5 (bits:read (reader [1 0 1 1]) 3))
        "read takes as many bits as it is given")
(assert (= 0 (bits:read (reader [1]) 0)) "and none for a width of 0")

# ── simplicity and splicing ───────────────────────────────────────────
(assert (bits:simpler? [1 1] [0 0 0]) "fewer bits are simpler")
(assert (bits:simpler? [0 1 1] [1 0 0])
        "as many, and smaller at the first that differs")
(assert (not (bits:simpler? [0 1] [0 1]))
        "a sequence is not simpler than itself")
(assert (= [1 0 1] (bits:spliced [1 1 1 1] 1 3 [0]))
        "a splice replaces the bits from start up to end")
(assert (= [1 1] (bits:spliced [1 0 0 1] 1 3 [])) "or deletes them")
(assert (and (bits:bit-array? [0 1]) (bits:bit-array? []))
        "an array of 0 and 1 is a bit array")
(assert (not (or (bits:bit-array? [0 2]) (bits:bit-array? "01")
                 (bits:bit-array? [0.0]))) "and nothing else is")

# ── the record of a replay ────────────────────────────────────────────
(let [src (source:replay [1 1 0 1 1 0])
      draw (src :draw)
      a (draw 3)
      b (draw 4)
      record ((src :record))]
  (assert (= [1 2] [a b]) "the bits 1 1 name 3, so the first draw reads on")
  (assert (= [1 1 0 1 1 0] (record :bits)) "the record keeps every bit read")
  (assert (= [3 4] (record :bounds)) "one bound for each draw")
  (assert (= [1 2] (record :choices)) "one choice")
  (assert (= [0 4] (record :starts)) "where its bits start")
  (assert (= [2 4] (record :accepted)) "where its accepted bits start")
  (assert (= [4 6] (record :ends)) "where they end")
  (assert (= [nil nil] (record :elements)) "and no elements for a single draw"))

(println "check-bits: all tests passed")
