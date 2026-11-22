(elle/epoch 18)
# audited: 2026-11-21
# Glean names a value by the slot that first receives it, a free name by its capture or its constant, and a pass by its slot.
# tools/glean/overview.md
#
# The counter-factual: a glean that named a value by its register would give
# use([:reg 2], 5) for x, and no pass, so the rules would never see `a` handed
# to the temporary slot the lowerer returns through.

(include-file "glean/helpers.lisp")
(def {:glean glean} ((import "../../tools/glean/glean")))

# (defn f [x] (def a @[x]) a), as the lowerer writes it: the parameter reloads
# into its own slot, and the result passes through a temporary slot.
(def f
  (function {:name 'f
             :arity [1 1]
             :params 1
             :slots ['x 'a nil]
             :blocks [(block 0
                        [{:kind :load-capture-raw :dst 0 :index 0}  # 0  x
                         {:kind :store-local :slot 0 :src 0}  # 1  its own slot
                         {:kind :const :dst 1 :value nil}  # 2
                         {:kind :store-local :slot 1 :src 1}  # 3  a constant
                         {:kind :load-local :dst 2 :slot 0}  # 4  x
                         {:kind :make-array-mut :dst 3 :elements [2] :region 7}  # 5
                         {:kind :store-local :slot 1 :src 3}  # 6  names it a
                         {:kind :load-local :dst 4 :slot 1}  # 7  a
                         {:kind :store-local :slot 2 :src 4}  # 8  a passes
                         {:kind :load-local :dst 5 :slot 2}]  # 9
                        {:kind :return :value 5})]}))  # 10

(def answer (glean [f]))
(assert (= (get (function-of answer 0) :name) 'f)
        "the answer keeps the function's name")
(assert (= (get (function-of answer 0) :id) 0) "and its closure id")
(expect-facts answer 0
              {:next [;(steps 10) (exit 10)]
               :param [['x 0]]
               :alloc [['a 5]]
               :operand [['x 'a 5]]
               :use [['x 5] [[:slot 2] 10]]
               :pass [['a [:slot 2] 8]]
               :return [[[:slot 2] 10]]})
(assert (empty? (get (function-of answer 0) :marks)) "f holds no unruled kind")

# A function with a capture, a synthetic capture and a heap constant. Index 2
# is past the two captures, so it loads slot 0, the parameter y.
(def h
  (function {:name 'h
             :arity [1 1]
             :params 1
             :captures ['arr :synthetic]
             :slots ['y nil]
             :blocks [(block 0
                        [{:kind :load-capture-raw :dst 0 :index 2}  # 0  y
                         {:kind :store-local :slot 0 :src 0}  # 1
                         {:kind :load-capture :dst 1 :index 0}  # 2  arr
                         {:kind :first :dst 2 :pair 1}  # 3  read out of arr
                         {:kind :store-local :slot 1 :src 2}  # 4  names it [:slot 1]
                         {:kind :load-capture-raw :dst 3 :index 1}  # 5  [:capture 1]
                         {:kind :value-const :dst 4 :value [1 2]}  # 6  [:const 6]
                         {:kind :make-array-mut
                          :dst 5
                          :elements [3 4 0]
                          :region 9}]  # 7
                        {:kind :return :value 5})]}))  # 8

(expect-facts (glean [h]) 0
              {:next [;(steps 8) (exit 8)]
               :param [['y 0]]
               :free ['arr [:capture 1] [:const 6]]
               :captured ['arr [:capture 1]]
               :read [[[:slot 1] 'arr 3]]
               :alloc [[[:reg 5] 7]]
               :operand [[[:capture 1] [:reg 5] 7] [[:const 6] [:reg 5] 7]
                         ['y [:reg 5] 7]]
               :use [['arr 3] [[:capture 1] 7] [[:const 6] 7] ['y 7]
                     [[:reg 5] 8]]
               :return [[[:reg 5] 8]]})

(println "glean-names: ok")
