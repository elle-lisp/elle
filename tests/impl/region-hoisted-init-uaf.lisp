(elle/epoch 15)
# audited: 2026-11-12
# A reassigned var whose init is a hoisted loop constant counts that init, so no pass frees the constant.
# docs/impl/hoist.md
#
# The sidecar arms guardfree and keeps the JIT off, so an over-free faults at
# the next pass's read of the constant. The value faces of the same shapes live
# in tests/lang/literal-var.lisp, where a freed constant reads correctly until
# its memory is reused.
#
# The counter-factual: a cell that took the constant by donation would release
# the constant's one reference at its first overwrite. The constant is bound
# once, before the loop, so the next pass starts its var from a freed value.

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

(defn collected [n xs]
  "n copies of xs, each built from the literal [] by append."
  (def out @[])
  (repeat n
    (var acc [])
    (each x in xs
      (assign acc (append acc [x])))
    (push out acc))
  (freeze out))

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

(assert (= (tagged 3) ["x0" "x1" "x2"]) "the string literal outlives each pass")
(assert (= (collected 3 [1 2]) [[1 2] [1 2] [1 2]])
        "the array literal outlives each outer pass")
(assert (= (pairs-twice [1 2 3 4 5]) [[1 2] [3 4] [1 2] [3 4]])
        "the array literal outlives each inner restart")

(println "region-hoisted-init-uaf: ok")
