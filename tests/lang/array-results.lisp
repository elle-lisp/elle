(elle/epoch 15)
# audited: 2026-10-26
# The sequence operators keep the collection's type: an array gives an array and an @array an @array, an empty input included.
# docs/arrays.md
#
# The trap: each operator once tested for an empty collection before it tested
# for an array, so an empty array took the list arm and gave (). A test over
# non-empty inputs alone never reaches that arm, so each input below comes empty
# and non-empty. The counter-factual for the rest is the old array arm, which
# filled an @array and returned it unfrozen whatever the input was.

(def ops
  [["flatten" (fn [c] (flatten c))] ["take" (fn [c] (take 1 c))]
   ["take-while" (fn [c] (take-while odd? c))]
   ["drop-while" (fn [c] (drop-while odd? c))]
   ["distinct" (fn [c] (distinct c))]
   ["mapcat" (fn [c] (mapcat (fn [x] [x]) c))]
   ["map-indexed" (fn [c] (map-indexed (fn [i x] x) c))]
   ["partition" (fn [c] (partition 2 c))]
   ["interpose" (fn [c] (interpose :sep c))] ["zip" (fn [c] (zip c c))]])

(defn keeps-type [label result want]
  (assert (= (type-of result) want)
          (string label ": gives " want ", got " (type-of result))))

(each [name op] in ops
  (keeps-type (string name " over [1 2]") (op [1 2]) :array)
  (keeps-type (string name " over @[1 2]") (op @[1 2]) :@array)
  (keeps-type (string name " over []") (op []) :array)
  (keeps-type (string name " over @[]") (op @[]) :@array)
  (assert (empty? (op [])) (string name " over []: gives an empty array"))
  (assert (empty? (op @[])) (string name " over @[]: gives an empty @array")))

## ── take keeps the leading elements, at any count ─────────────────────
(assert (= (take 2 [1 2 3]) [1 2]) "take 2 of an array")
(assert (= (take 5 [1 2 3]) [1 2 3]) "take more than the array holds")
(keeps-type "take 0 of an array" (take 0 [1 2 3]) :array)
(keeps-type "take 2 of an @array" (take 2 @[1 2 3]) :@array)
(assert (= (take 2 (list 1 2 3)) (list 1 2)) "a list still gives a list")

## ── each chunk of a partition follows the input ───────────────────────
(each chunk in (partition 2 [1 2 3])
  (keeps-type "a chunk of an array" chunk :array))
(each chunk in (partition 2 @[1 2 3])
  (keeps-type "a chunk of an @array" chunk :@array))
(assert (= (partition 2 [1 2 3]) [[1 2] [3]]) "partition splits an array")

## ── zip follows its first input ───────────────────────────────────────
(keeps-type "zip with an array first" (zip [1 2] @[3 4]) :array)
(keeps-type "zip with an @array first" (zip @[1 2] [3 4]) :@array)
(assert (= (type-of (zip (list 1 2) [3 4])) :list) "zip with a list first")
(assert (= (zip [1 2] [3 4]) [(list 1 3) (list 2 4)]) "zip pairs as lists")

## ── interpose gives a new @array for one element ──────────────────────
(let* [src @[1]
       ip (interpose :sep src)]
  (assert (= ip @[1]) "interpose over @[1]: gives the one element")
  (push ip 2)
  (assert (= src @[1]) "interpose over @[1]: the result is not the input"))
