(elle/epoch 15)
# audited: 2026-10-24
# The sequence operators answer a new @array for an array input of any length, the empty one included.
# docs/traits.md
#
# The trap: each operator tests for an empty collection before it tests for an
# array, so an empty array takes the list arm and answers (). A caller then
# hands that list to `append` beside an array, or to `freeze`, and both raise.
# A test over non-empty inputs alone never reaches that arm.

(def ops
  [["flatten" (fn [c] (flatten c))] ["take-while" (fn [c] (take-while odd? c))]
   ["drop-while" (fn [c] (drop-while odd? c))]
   ["distinct" (fn [c] (distinct c))]
   ["mapcat" (fn [c] (mapcat (fn [x] [x]) c))]
   ["map-indexed" (fn [c] (map-indexed (fn [i x] x) c))]
   ["partition" (fn [c] (partition 2 c))]
   ["interpose" (fn [c] (interpose :sep c))]])

(defn answers-empty-array [label result]
  (assert (= (type-of result) :@array)
          (string label ": answers an @array, got " (type-of result)))
  (assert (empty? result) (string label ": answers an empty @array")))

(each [name op] in ops
  (answers-empty-array (string name " over []") (op []))
  (answers-empty-array (string name " over @[]") (op @[])))

## ── The answer works where an array does ──────────────────────────────
(each [name op] in ops
  (assert (= (append (op []) [1]) [1])
          (string name ": the answer over [] appends an array"))
  (assert (= (type-of (freeze (op @[]))) :array)
          (string name ": the answer over @[] freezes to an array")))

## ── interpose answers a new @array for one element ───────────────────
(assert (= (type-of (interpose :sep [1])) :@array)
        "interpose over [1]: answers an @array")
(let* [src @[1]
       ip (interpose :sep src)]
  (assert (= ip @[1]) "interpose over @[1]: answers the one element")
  (push ip 2)
  (assert (= src @[1]) "interpose over @[1]: the answer is not the input"))
