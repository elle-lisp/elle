(elle/epoch 18)
# audited: 2026-11-24
# compile/lir shows each array rest slicing with the instruction of its scrutinee's kind.
# docs/impl/region/anchors.md
#
# array-slice-from slices an array and array-mut-slice-from an @array. A match
# pattern names the kind it tests, and a yielding guard moves the match onto
# the sequential lowering. A binding form takes either kind, so it tests
# is-array-mut and takes both slices unless the value's type is proven.
#
# The counter-factual: one slice for both kinds reads no array-slice-from in
# any function here, and a branch that ignores a proven type reads both slices
# in dm and di.

(def unit
  (compile/lir (string "(defn mi [xs] (match xs [a & r] r _ nil))\n"
                       "(defn mm [xs] (match xs @[a & r] r _ nil))\n"
                       "(defn qi [xs] (match xs [a & r] when (yield a) r _ nil))\n"
                       "(defn qm [xs] (match xs @[a & r] when (yield a) r _ nil))\n"
                       "(defn du [xs] (def [a & r] xs) r)\n"
                       "(defn da [xs] (def @[a & r] xs) r)\n"
                       "(defn lu [xs] (let [[a & r] xs] r))\n"
                       "(defn dm [n] (def [a & r] @[n n]) r)\n"
                       "(defn di [n] (def [a & r] [n n]) r)\n") "<lir-slices>"))

(defn slicing [name]
  "How many array-slice-from, array-mut-slice-from and is-array-mut NAME's function holds."
  (def fun (find (fn [f] (= (get f :name) name)) unit))
  (def kinds
    (mapcat (fn [b] (map (fn [i] (get i :kind)) (get b :instrs)))
            (get fun :blocks)))
  (map (fn [kind] (length (filter (fn [k] (= k kind)) kinds)))
       [:array-slice-from :array-mut-slice-from :is-array-mut]))

(assert (= (slicing 'mi) [1 0 0]) "a match on [a & r] slices an array")
(assert (= (slicing 'mm) [0 1 1]) "a match on @[a & r] slices an @array")
(assert (= (slicing 'qi) [1 0 0])
        "a sequential match on [a & r] slices an array")
(assert (= (slicing 'qm) [0 1 1])
        "a sequential match on @[a & r] slices an @array")
(assert (= (slicing 'du) [1 1 1])
        "a def over a value of unknown kind takes both slices")
(assert (= (slicing 'da) [1 1 1])
        "a def with @[a & r] over a value of unknown kind takes both slices")
(assert (= (slicing 'lu) [1 1 1])
        "a let over a value of unknown kind takes both slices")
(assert (= (slicing 'dm) [0 1 0])
        "a def over a proven @array takes its slice alone")
(assert (= (slicing 'di) [1 0 0])
        "a def over a proven array takes its slice alone")

(println "lir-slices: ok")
