(elle/epoch 18)
# audited: 2026-11-24
# The present release of a counted region is the step that takes the function's count of it to zero.
# tools/glean/present.md
#
# The counter-factual: a reader that took each release as a release of the
# value would release a twice in c2, where a retain stands between. One that
# minted a new region at every allocation would leave a held in c3, and one
# that read every DecrefRegion as a count step would find c4's second release
# over. A Return that released would release x in c5, and a read or a capture
# counted like a made value would release in c6 where the function holds none.

(include-file "glean/helpers.lisp")
(def present (get ((import "../../tools/glean/present")) :present))

# A release by value at the count's last reference.
(def c1
  (function {:slots ['a]
             :blocks [(basic-block 0
                                   [(fresh-array 0 3)  # 0  a
                                    (store-slot 0 0)  # 1
                                    (load-slot 1 0)  # 2
                                    (release-value 1)  # 3
                                    (nil-const 2)]  # 4
                                   (return-of 2))]}))  # 5
(expect-present (present [c1]) 0 {:released [['a 3]]})

# A retain and its release leave the count where it was.
(def c2
  (function {:slots ['a]
             :blocks [(basic-block 0
                                   [(fresh-array 0 3)  # 0  a, count 1
                                    (store-slot 0 0)  # 1
                                    (load-slot 1 0)  # 2
                                    {:kind :incref-value-region :src 1}  # 3  count 2
                                    (load-slot 2 0)  # 4
                                    (release-value 2)  # 5  count 1
                                    (load-slot 3 0)  # 6
                                    (release-value 3)  # 7  count 0
                                    (nil-const 4)]  # 8
                                   (return-of 4))]}))  # 9
(expect-present (present [c2]) 0 {:released [['a 7]]})

# Two allocations name static region 5, so the second reuses the first's
# region, and one DecrefRegion releases both values.
(def c3
  (function {:slots ['a 'b]
             :blocks [(basic-block 0
                                   [{:kind :const :dst 0 :value 1}  # 0
                                    {:kind :list
                                     :dst 1
                                     :head 0
                                     :tail 0
                                     :region 5}  # 1  a
                                    (store-slot 0 1)  # 2
                                    (load-slot 2 0)  # 3
                                    {:kind :list
                                     :dst 3
                                     :head 2
                                     :tail 0
                                     :region 5}  # 4  b
                                    (store-slot 1 3)  # 5
                                    {:kind :decref-region :region-id 5}  # 6
                                    (nil-const 4)]  # 7
                                   (return-of 4))]}))  # 8
(expect-present (present [c3]) 0 {:released [['a 6] ['b 6]]})

# A DecrefRegion takes its static region's mapping, so a second one does nothing.
(def c4
  (function {:slots ['a]
             :blocks [(basic-block 0
                                   [(fresh-array 0 3)  # 0  a
                                    (store-slot 0 0)  # 1
                                    {:kind :decref-region :region-id 3}  # 2
                                    {:kind :decref-region :region-id 3}  # 3
                                    (nil-const 1)]  # 4
                                   (return-of 1))]}))  # 5
(expect-present (present [c4]) 0 {:released [['a 2]]})

# The parameter's count goes to the caller, and a stays counted at the exit.
(def c5
  (function {:params 1
             :arity [1 1]
             :slots ['x 'a]
             :blocks [(basic-block 0
                                   [(fresh-array 0 3)  # 0  a
                                    (store-slot 1 0)  # 1
                                    (load-slot 1 0)]  # 2  x
                                   (return-of 1))]}))  # 3
(expect-present (present [c5]) 0 {:handed [['x 3]] :held [['a 3]]})

# A call result is counted. A capture, a heap constant and a read are not, so a
# release of each is over.
(def c6
  (function {:captures ['k]
             :slots ['r]
             :blocks [(basic-block 0
                                   [{:kind :value-const :dst 0 :value length}  # 0
                                    {:kind :load-capture :dst 1 :index 0}  # 1  k
                                    (put (invoke :call 2 0 [1]) :region 7)  # 2  r
                                    (store-slot 0 2)  # 3
                                    (load-slot 3 0)  # 4
                                    (release-value 3)  # 5
                                    (release-value 1)  # 6
                                    {:kind :value-const :dst 4 :value [1 2]}  # 7
                                    (release-value 4)  # 8
                                    {:kind :first :dst 5 :pair 1}  # 9
                                    (release-value 5)  # 10
                                    (nil-const 6)]  # 11
                                   (return-of 6))]}))  # 12
(expect-present (present [c6]) 0
                {:released [['r 5]] :over [['k 6] [[:const 7] 8] [[:reg 5] 10]]})

(println "glean-present-counts: ok")
