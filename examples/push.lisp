(elle/epoch 9)

# ── push on mutable collections (existing behavior) ──────────

(let [a @[1 2]]
  (push a 3)
  (assert (= a @[1 2 3]) "push @array mutates in place"))

(let [s @"ab"]
  (push s "c")
  (assert (= s @"abc") "push @string mutates in place"))

(let [b @b[1 2]]
  (push b 3)
  (assert (= b @b[1 2 3]) "push @bytes mutates in place"))

# ── push on immutable collections (new behavior) ─────────────

(let [a [1 2]
      a2 (push a 3)]
  (assert (= a [1 2]) "push array: original unchanged")
  (assert (= a2 [1 2 3]) "push array: returns new array"))

(let [s "ab"
      s2 (push s "c")]
  (assert (= s "ab") "push string: original unchanged")
  (assert (= s2 "abc") "push string: returns new string"))

(let [b b[1 2]
      b2 (push b 3)]
  (assert (= b b[1 2]) "push bytes: original unchanged")
  (assert (= b2 b[1 2 3]) "push bytes: returns new bytes"))

(println "push: all tests passed")
