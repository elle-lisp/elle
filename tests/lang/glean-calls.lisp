(elle/epoch 18)
# audited: 2026-11-23
# Glean names each callee, gives a call its arguments, reads a native's :spec, and marks a native that declares none.
# tools/glean/overview.md
#
# The counter-factual: fiber/resume resumes its fiber and passes its value to
# the child, so a glean that gave it only call and arg facts would leave the
# rules holding w after the child may have released it. A glean that gave no
# store for a stored parameter would refuse no store of a borrowed value.

(include-file "glean/helpers.lisp")
(def glean (quiet (get ((import "../../tools/glean/glean")) :glean)))

(defn invoke [kind dst func args]
  "A call instruction of KIND, as the lowerer writes a call it checked."
  {:kind kind
   :dst dst
   :func func
   :args args
   :arity-checked true
   :region 0
   :moves []})

# (defn c [f] (def w @[]) (def r (fiber/resume f w)) (def v (fiber/value f))
#   (list 7 v r))
(def c
  (function {:name 'c
             :arity [1 1]
             :params 1
             :slots ['f 'w 'r 'v]
             :blocks [(basic-block 0
                                   [{:kind :load-capture-raw :dst 0 :index 0}  # 0  f
                                    {:kind :store-local :slot 0 :src 0}  # 1
                                    {:kind :make-array-mut
                                     :dst 1
                                     :elements []
                                     :region 3}  # 2
                                    {:kind :store-local :slot 1 :src 1}  # 3  w
                                    {:kind :value-const
                                     :dst 2
                                     :value fiber/resume}  # 4
                                    (invoke :suspending-call 3 2 [0 1])  # 5
                                    {:kind :store-local :slot 2 :src 3}  # 6  r
                                    {:kind :value-const
                                     :dst 4
                                     :value fiber/value}  # 7
                                    (invoke :call 5 4 [0])  # 8
                                    {:kind :store-local :slot 3 :src 5}  # 9  v
                                    {:kind :const :dst 6 :value 7}  # 10
                                    {:kind :value-const :dst 7 :value list}  # 11
                                    (invoke :call 8 7 [6 5 3])]  # 12
                                   {:kind :return :value 8})]}))  # 13

(def natives (glean [c]))
(expect-facts natives 0
              {:next [;(steps 13) (exit 13)]
               :param [['f 0]]
               :alloc [['w 2]]
               :call [['r "fiber/resume" 5] ['v "fiber/value" 8]
                      [[:reg 8] "list" 12]]
               :arg [['f 0 5] ['w 1 5] ['f 0 8] [[:imm 10] 0 12] ['v 1 12]
                     ['r 2 12]]
               :immediate [[:imm 10]]
               :resume [['f 5]]
               :cross [['w 5]]
               :payload [['r 'f 5] ['v 'f 8]]
               :use [['f 5] ['w 5] ['f 8] [[:imm 10] 12] ['v 12] ['r 12]
                     [[:reg 8] 13]]
               :return [[[:reg 8] 13]]})
(assert (empty? (get (function-of natives 0) :marks))
        "each native c calls declares its spec")
(assert (empty? (get natives :closures)) "and c meets no closure constant")

(def one (fn [x] x))
(def two (fn [y] y))

# get declares no :spec, so its call marks the function and gives use alone.
# The closure constants one and two are callees by their index in the unit's
# table, and the last call's callee is the parameter t.
(def d
  (function {:name 'd
             :arity [1 1]
             :params 1
             :slots ['t]
             :blocks [(basic-block 0
                                   [{:kind :load-capture-raw :dst 0 :index 0}  # 0  t
                                    {:kind :store-local :slot 0 :src 0}  # 1
                                    {:kind :value-const :dst 1 :value get}  # 2
                                    {:kind :const :dst 2 :value :k}  # 3
                                    (invoke :call 3 1 [0 2])  # 4
                                    {:kind :value-const :dst 4 :value one}  # 5
                                    (invoke :call 5 4 [3])  # 6
                                    {:kind :value-const :dst 6 :value two}  # 7
                                    (invoke :call 7 6 [5])  # 8
                                    {:kind :value-const :dst 8 :value one}  # 9
                                    (invoke :call 9 8 [7])  # 10
                                    (invoke :call 10 0 [9])]  # 11
                                   {:kind :return :value 10})]}))  # 12

# The second function of the unit meets two again.
(def e
  (function {:id 1
             :name 'e
             :blocks [(basic-block 0
                                   [{:kind :value-const :dst 0 :value two}  # 0
                                    {:kind :tail-call
                                     :dst 1
                                     :func 0
                                     :args []
                                     :arity-checked true
                                     :region 0
                                     :defer-callee-release false
                                     :deferred-release-slot nil
                                     :borrowed-arg-slots []
                                     :late-mints []}]  # 1
                                   {:kind :return :value 1})]}))  # 2

(def closures (glean [d e]))
(expect-facts closures 0
              {:next [;(steps 12) (exit 12)]
               :param [['t 0]]
               :call [[[:reg 5] [:fn 0] 6] [[:reg 7] [:fn 1] 8]
                      [[:reg 9] [:fn 0] 10] [[:reg 10] 't 11]]
               :arg [[[:reg 3] 0 6] [[:reg 5] 0 8] [[:reg 7] 0 10]
                     [[:reg 9] 0 11]]
               :immediate [[:imm 3]]
               :use [['t 4] [[:imm 3] 4] [[:reg 3] 6] [[:reg 5] 8] [[:reg 7] 10]
                     ['t 11] [[:reg 9] 11] [[:reg 10] 12]]
               :return [[[:reg 10] 12]]})
(assert (= (get (function-of closures 0) :marks) {"get" 1})
        "a call to a native with no :spec marks the function with the native's name")
(expect-facts closures 1
              {:next [[0 1] [1 2] (exit 1) (exit 2)]
               :call [[[:reg 1] [:fn 1] 1]]
               :use [[[:reg 1] 2]]
               :return [[[:reg 1] 1] [[:reg 1] 2]]})
(assert (= (get (function-of closures 1) :name) 'e) "the second struct is e")
(def met (get closures :closures))
(assert (and (= (length met) 2) (identical? (get met 0) one)
             (identical? (get met 1) two))
        "the unit's closure constants, each once, in the order glean meets them")

# A call on an immediate: its callee is no function, and the value it names is
# an immediate.
(def k
  (function {:name 'k
             :blocks [(basic-block 0
                                   [{:kind :const :dst 0 :value :k}  # 0
                                    (invoke :call 1 0 [])]  # 1
                                   {:kind :return :value 1})]}))  # 2

(def [immediate-ok? immediate-err] (protect (glean [k])))
(assert (not immediate-ok?)
        "glean refuses a call whose callee names an immediate")
(assert (= :glean-error (get immediate-err :error)) "with an error of its own")

# (defn s [a x] (def w (@array 1 x)) (insert a 0 x) (insert 5 0 x) w)
# @array stores each argument into its fresh result, the immediate 1 too, and
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

(println "glean-calls: ok")
