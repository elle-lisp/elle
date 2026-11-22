(elle/epoch 18)
# audited: 2026-11-21
# Glean names nothing for a slot that receives no value, and names a value by its first slot in view order, not by label.
# tools/glean/overview.md
#
# The counter-factual: a glean that walked the points once would find slot 1
# of `w` empty at its load, ahead of the store in view order, and give no use
# of the value the loop makes. One that took the blocks by label would name
# the array in `v` b, not a.

(include-file "glean/helpers.lisp")
(def {:glean glean} ((import "../../tools/glean/glean")))

# Slot 0 receives a constant alone, and slot 2 only what slot 0 holds, so a
# load from either names nothing. The sum is a computed immediate that no slot
# receives, and the compare's value is the first value slot 1 receives.
(def g
  (function {:name 'g
             :slots [nil nil nil]
             :blocks [(block 0
                        [{:kind :const :dst 0 :value 1}  # 0
                         {:kind :const :dst 1 :value 2}  # 1
                         {:kind :bin-op
                          :dst 2
                          :op :add
                          :lhs 0
                          :rhs 1
                          :proof :int}  # 2
                         {:kind :const :dst 3 :value true}  # 3
                         {:kind :store-local :slot 0 :src 3}  # 4
                         {:kind :load-local :dst 4 :slot 0}  # 5
                         {:kind :store-local :slot 2 :src 4}  # 6
                         {:kind :load-local :dst 5 :slot 2}  # 7
                         {:kind :compare
                          :dst 6
                          :op :eq
                          :lhs 5
                          :rhs 2
                          :proof :unproven}  # 8
                         {:kind :store-local :slot 1 :src 6}  # 9
                         {:kind :load-local :dst 7 :slot 1}]  # 10
                        {:kind :branch :cond 7 :then-label 1 :else-label 2})  # 11
                      (block 1
                        []
                        {:kind :return :value 2})  # 12
                      (block 2
                        []
                        {:kind :return :value 5})]}))  # 13

(expect-facts (glean [g]) 0
              {:next [;(steps 11) [11 12] [11 13] (exit 12) (exit 13)]
               :use [[[:reg 2] 8] [[:slot 1] 11] [[:reg 2] 12]]
               :return [[[:reg 2] 12]]})

# The blocks in view order are 0, 2, 1. The array made at 2 is stored in
# block 2 before block 1, so its name is a, and block 1 passes it to b. The
# array made at 6 is first stored into acc, which holds another value already,
# so it is acc made anew.
(def v
  (function {:name 'v
             :slots ['acc 'b 'a]
             :blocks [(block 0
                        [{:kind :make-array-mut :dst 0 :elements [] :region 3}  # 0
                         {:kind :store-local :slot 0 :src 0}  # 1  acc
                         {:kind :make-array-mut :dst 1 :elements [] :region 4}  # 2
                         {:kind :is-nil :dst 2 :src 1}]  # 3
                        {:kind :branch :cond 2 :then-label 1 :else-label 2})  # 4
                      (block 2
                        [{:kind :store-local :slot 2 :src 1}  # 5  a
                         {:kind :make-array-mut :dst 3 :elements [] :region 5}  # 6
                         {:kind :store-local :slot 0 :src 3}]  # 7  acc anew
                        {:kind :jump :target 1})  # 8
                      (block 1
                        [{:kind :store-local :slot 1 :src 1}  # 9  a to b
                         {:kind :load-local :dst 4 :slot 0}]  # 10
                        {:kind :return :value 4})]}))  # 11

(expect-facts (glean [v]) 0
              {:next [[0 1] [1 2] [2 3] [3 4] [4 9] [4 5] [5 6] [6 7] [7 8]
                      [8 9] [9 10] [10 11] (exit 11)]
               :alloc [['acc 0] ['a 2] ['acc 6]]
               :use [['a 3] [[:reg 2] 4] ['acc 11]]
               :pass [['a 'b 9]]
               :return [['acc 11]]})

# A loop whose body makes the value its test reads: slot 1 receives a value
# only at 5, after the load at 1 in view order.
(def w
  (function {:name 'w
             :slots ['i nil]
             :blocks [(block 0
                        []
                        {:kind :jump :target 1})  # 0
                      (block 1
                        [{:kind :load-local :dst 0 :slot 1}  # 1
                         {:kind :is-nil :dst 1 :src 0}]  # 2
                        {:kind :branch :cond 1 :then-label 2 :else-label 3})  # 3
                      (block 2
                        [{:kind :make-array-mut :dst 2 :elements [] :region 6}  # 4
                         {:kind :store-local :slot 1 :src 2}]  # 5
                        {:kind :jump :target 1})  # 6
                      (block 3
                        []
                        {:kind :return :value 0})]}))  # 7

(expect-facts (glean [w]) 0
              {:next [[0 1] [1 2] [2 3] [3 4] [3 7] [4 5] [5 6] [6 1] (exit 7)]
               :alloc [[[:slot 1] 4]]
               :use [[[:slot 1] 2] [[:reg 1] 3] [[:slot 1] 7]]
               :return [[[:slot 1] 7]]})

(println "glean-slots: ok")
