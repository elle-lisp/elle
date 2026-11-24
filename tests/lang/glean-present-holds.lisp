(elle/epoch 18)
# audited: 2026-11-24
# Which values the function counted at birth, holds on a path into each step, and has ended there.
# tools/glean/present.md
#
# The counter-factual: a reader that noted the state after each step would
# hold a at 0 in h1 and end it at 3. One that counted a read or a capture at
# birth, or held a region it counts zero times, would answer k and [:reg 1] in
# h2. One that held every owned region would hold c past 11 in h3, and one
# that held counted regions alone would stop holding b at 9. One that forgot
# the regions the activation keeps for its exit would not hold r at 7 in h4.
# One that counted every value an allocation names would count b in h5.

(include-file "glean/helpers.lisp")
(def present (get ((import "../../tools/glean/present")) :present))

# a is held from its birth to its release, and has ended after it.
(def h1
  (function {:slots ['a]
             :blocks [(basic-block 0
                                   [(fresh-array 0 3)  # 0  a
                                    (store-slot 0 0)  # 1
                                    (load-slot 1 0)  # 2
                                    (release-value 1)  # 3
                                    (nil-const 2)]  # 4
                                   (return-of 2))]}))  # 5
(expect-holds (present [h1]) 0
              {:counted ['a] :holding (span 'a 1 3) :ended (span 'a 4 5)})

# The parameter x is counted from the entry. The capture k and the read
# [:reg 1] begin with no count, and the retain at 2 holds the read.
(def h2
  (function {:params 1
             :arity [1 1]
             :captures ['k]
             :slots ['x]
             :blocks [(basic-block 0
                                   [{:kind :load-capture :dst 0 :index 0}  # 0  k
                                    {:kind :first :dst 1 :pair 0}  # 1  [:reg 1]
                                    {:kind :incref-value-region :src 1}  # 2
                                    (load-slot 2 0)]  # 3  x
                                   (return-of 2))]}))  # 4
(expect-holds (present [h2]) 0
              {:counted ['x] :holding [;(span 'x 0 4) [[:reg 1] 3] [[:reg 1] 4]]})

# a owns b from 8, so b is held while a is, and ends with it. k's region owns
# c from 11, and the function counts k zero times, so c is neither held nor
# ended after 11.
(def h3
  (function {:captures ['k]
             :slots ['a 'b 'c]
             :blocks [(basic-block 0
                                   [(fresh-array 0 3)  # 0  a
                                    (store-slot 0 0)  # 1
                                    (fresh-array 1 4)  # 2  b
                                    (store-slot 1 1)  # 3
                                    (fresh-array 2 5)  # 4  c
                                    (store-slot 2 2)  # 5
                                    (load-slot 3 0)  # 6
                                    (load-slot 4 1)  # 7
                                    {:kind :adopt-region :parent 3 :child 4}  # 8
                                    {:kind :load-capture :dst 5 :index 0}  # 9  k
                                    (load-slot 6 2)  # 10
                                    {:kind :adopt-region :parent 5 :child 6}  # 11
                                    (load-slot 7 0)  # 12
                                    (release-value 7)  # 13
                                    (nil-const 8)]  # 14
                                   (return-of 8))]}))  # 15
(expect-holds (present [h3]) 0
              {:counted ['a 'b 'c]
               :holding [;(span 'a 1 13) ;(span 'b 3 13) ;(span 'c 5 11)]
               :ended [;(span 'a 14 15) ;(span 'b 14 15)]})

# Each pass makes r at 1, and the activation adopts it at 4 or the release at
# 6 ends it. A pass after an adoption makes r again while the activation still
# keeps the last one for its exit, so after the release on that pass r is both
# held and ended at 7.
(def r [:reg 0])
(def h4
  (function {:blocks [(basic-block 0 [] (jump-to 1))  # 0
                      (basic-block 1
                                   [(fresh-array 0 3)  # 1  r
                                    {:kind :const :dst 1 :value true}]  # 2
                                   (branch-on 1 2 3))  # 3
                      (basic-block 2
                                   [{:kind :adopt-into-activation :child 0}]  # 4
                                   (jump-to 4))  # 5
                      (basic-block 3
                                   [(release-value 0)]  # 6
                                   (jump-to 4))  # 7
                      (basic-block 4
                                   [{:kind :const :dst 2 :value true}]  # 8
                                   (branch-on 2 1 5))  # 9
                      (basic-block 5
                                   [(nil-const 3)]  # 10
                                   (return-of 3))]}))  # 11
(expect-holds (present [h4]) 0
              {:counted [r] :holding (span r 1 11) :ended [[r 1] ;(span r 7 11)]})

# Both lists name static region 5, so b reuses a's region, which the
# activation owns by then: b is held, and was not counted at its birth.
(def h5
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
                                    {:kind :adopt-into-activation :child 2}  # 4
                                    {:kind :list
                                     :dst 3
                                     :head 2
                                     :tail 0
                                     :region 5}  # 5  b
                                    (store-slot 1 3)  # 6
                                    (nil-const 4)]  # 7
                                   (return-of 4))]}))  # 8
(expect-holds (present [h5]) 0
              {:counted ['a] :holding [;(span 'a 2 8) ;(span 'b 6 8)]})

(println "glean-present-holds: ok")
