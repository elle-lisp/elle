(elle/epoch 18)
# audited: 2026-11-23
# Glean gives store for each argument a native's :spec stores into a place, and none into an immediate.
# tools/glean/overview.md
#
# The counter-factual: a glean that gave no store for a stored parameter would
# refuse no store of a borrowed value.

(include-file "glean/helpers.lisp")
(def glean (quiet (get ((import "../../tools/glean/glean")) :glean)))

# (defn s [a x] (def w (@array 1 x)) (insert a 0 x) (insert 5 0 x) w)
# @array makes w and stores each argument into it, the immediate 1 too, and
# insert stores its value into its array. The last insert's array is an
# immediate, which insert refuses before it stores, so it gives no store.
(def s
  (function {:name 's
             :arity [2 2]
             :params 2
             :slots ['a 'x 'w]
             :blocks [(basic-block 0
                                   [{:kind :load-capture-raw :dst 0 :index 0}  # 0  a
                                    {:kind :store-local :slot 0 :src 0}  # 1
                                    {:kind :load-capture-raw :dst 1 :index 1}  # 2  x
                                    {:kind :store-local :slot 1 :src 1}  # 3
                                    {:kind :const :dst 2 :value 1}  # 4
                                    {:kind :value-const :dst 3 :value @array}  # 5
                                    (invoke :call 4 3 [2 1])  # 6
                                    {:kind :store-local :slot 2 :src 4}  # 7  w
                                    {:kind :value-const :dst 5 :value insert}  # 8
                                    {:kind :const :dst 6 :value 0}  # 9
                                    (invoke :call 7 5 [0 6 1])  # 10
                                    {:kind :const :dst 8 :value 5}  # 11
                                    (invoke :call 9 5 [8 6 1])]  # 12
                                   {:kind :return :value 4})]}))  # 13

(def stores (glean [s]))
(expect-facts stores 0
              {:next [;(steps 13) (exit 13)]
               :param [['a 0] ['x 1]]
               :immediate [[:imm 4] [:imm 9] [:imm 11]]
               :alloc [['w 6]]
               :call [['w "@array" 6] [[:reg 7] "insert" 10]
                      [[:reg 9] "insert" 12]]
               :arg [[[:imm 4] 0 6] ['x 1 6] ['a 0 10] [[:imm 9] 1 10] ['x 2 10]
                     [[:imm 11] 0 12] [[:imm 9] 1 12] ['x 2 12]]
               :store [[[:imm 4] 'w 6] ['x 'w 6] ['x 'a 10]]
               :use [[[:imm 4] 6] ['x 6] ['a 10] [[:imm 9] 10] ['x 10]
                     [[:imm 11] 12] [[:imm 9] 12] ['x 12] ['w 13]]
               :return [['w 13]]})
(assert (empty? (get (function-of stores 0) :marks))
        "@array and insert each declare their spec")

(println "glean-stores: ok")
