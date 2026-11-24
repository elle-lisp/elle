(elle/epoch 18)
# audited: 2026-11-24
# Each path of a function answers for itself, a loop ends when its states repeat, and a reader refuses what it cannot read.
# tools/glean/present.md
#
# The counter-factual: a reader that walked each point once would lose one of
# p1's arms. One that refused a DecrefRegion of a static region that maps
# nothing would refuse p2. One that replaced the region a loop made last
# without a word would lose its hold in the first loop. One that refused a
# region made again while a slot holds the last would refuse the second, and
# one that kept no second region would never refuse the third. One that kept
# a register no later step reads would refuse the fourth. A count that rises
# on every pass would never end the fifth.

(include-file "glean/helpers.lisp")
(def present (get ((import "../../tools/glean/present")) :present))

# The then arm releases a, and the else arm leaves it held at the exit.
(def p1
  (function {:slots ['a]
             :blocks [(basic-block 0
                                   [(fresh-array 0 3)  # 0  a
                                    (store-slot 0 0)  # 1
                                    {:kind :const :dst 1 :value true}]  # 2
                                   (branch-on 1 1 2))  # 3
                      (basic-block 1
                                   [(load-slot 2 0)  # 4
                                    (release-value 2)]  # 5
                                   (jump-to 3))  # 6
                      (basic-block 2 [] (jump-to 3))  # 7
                      (basic-block 3
                                   [(nil-const 3)]  # 8
                                   (return-of 3))]}))  # 9
(expect-present (present [p1]) 0 {:released [['a 5]] :held [['a 9]]})

# Static region 3 maps a region on the then arm alone, so the DecrefRegion at
# the join releases a there and does nothing on the else arm.
(def p2
  (function {:slots ['a]
             :blocks [(basic-block 0
                                   [{:kind :const :dst 0 :value true}]  # 0
                                   (branch-on 0 1 2))  # 1
                      (basic-block 1
                                   [(fresh-array 1 3)  # 2  a
                                    (store-slot 0 1)]  # 3
                                   (jump-to 3))  # 4
                      (basic-block 2 [] (jump-to 3))  # 5
                      (basic-block 3
                                   [{:kind :decref-region :region-id 3}  # 6
                                    (nil-const 2)]  # 7
                                   (return-of 2))]}))  # 8
(expect-present (present [p2]) 0 {:released [['a 6]]})

(defn looping [body]
  "A function that jumps to a loop whose BODY, from point 1, ends in a branch
   back to its head or on to a Return of nil. BODY holds n steps."
  (function {:captures ['k]
             :slots ['a]
             :blocks [(basic-block 0 [] (jump-to 1))  # 0
                      (basic-block 1
                                   [;body {:kind :const :dst 1 :value true}]  # n + 1
                                   (branch-on 1 1 2))  # n + 2
                      (basic-block 2
                                   [(nil-const 2)]  # n + 3
                                   (return-of 2))]}))  # n + 4

# Each pass makes a region at 1 again, and nothing holds the one the pass
# before made, so it is held there.
(expect-present (present [(looping [(fresh-array 0 3)])]) 0
                {:held [[[:reg 0] 1] [[:reg 0] 5]]})

# A reassigned binding: 1 makes the new value while slot a still holds the
# old, 3 releases the old through the slot, and 4 stores the new.
(expect-present (present [(looping [(fresh-array 0 3)  # 1  a
                                    (load-slot 5 0)  # 2
                                    (release-value 5)  # 3
                                    (store-slot 0 0)])])  # 4
                0 {:released [['a 3]] :held [['a 8]]})

# Slot 1 takes a's region and slot 2 takes slot 1's before 6 makes the next,
# and the next pass reads both, so on the third pass both regions 6 made are
# held.
(refused (fn []
           (present [(looping [(load-slot 7 2)  # 1
                               (load-slot 5 1)  # 2
                               (store-slot 2 5)  # 3
                               (load-slot 6 0)  # 4
                               (store-slot 1 6)  # 5
                               (fresh-array 0 3)  # 6
                               (store-slot 0 0)])]))  # 7
         :remade)

# Register 5 takes the old value and no later step reads it, so it does not
# hold the oldest region when 1 makes the next on the third pass.
(expect-present (present [(looping [(fresh-array 0 3)  # 1  a
                                    (load-slot 5 0)  # 2
                                    (store-slot 0 0)])])  # 3
                0 {:held [['a 1] ['a 7]]})

# Each pass retains k once more and never releases it.
(refused (fn []
           (present [(looping [{:kind :load-capture :dst 0 :index 0}
                               {:kind :incref-value-region :src 0}])]))
         :unbounded)

(refused (fn []
           (present [(function {:blocks [(basic-block 0 [{:kind :frobnicate}]
                                           (return-of 0))]})])) :unknown-kind)

(refused (fn []
           (present [(function {:blocks [(basic-block 0 [] (return-of 9))]})]))
         :unwritten)

(println "glean-present-paths: ok")
