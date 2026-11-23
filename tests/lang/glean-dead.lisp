(elle/epoch 18)
# audited: 2026-11-23
# A StoreLocal whose slot no load reads before the next store into it gives glean no pass, and still names its value.
# tools/glean/overview.md
#
# The counter-factual: a glean that read every StoreLocal as a pass would hand
# a to the result slot in f1, so the rules would end the fiber's hold on a
# before its return. One that counted only a LoadLocal as a load would drop
# the pass in f3, and one that took only a StoreLocal as a store would keep
# the pass in f4. One that looked down one branch would drop the pass in f5.

(include-file "glean/helpers.lisp")
(def {:glean glean} ((import "../../tools/glean/glean")))

# (defn f1 [] (def a @[]) a), as the lowerer writes it: the def's value goes to
# the body's result slot, which nothing loads.
(def f1
  (function {:name 'f1
             :slots ['a nil]
             :blocks [(basic-block 0
                                   [{:kind :make-array-mut
                                     :dst 0
                                     :elements []
                                     :region 3}  # 0
                                    {:kind :store-local :slot 0 :src 0}  # 1  a
                                    {:kind :load-local :dst 1 :slot 0}  # 2
                                    {:kind :store-local :slot 1 :src 1}  # 3  dead
                                    {:kind :load-local :dst 2 :slot 0}]  # 4
                                   {:kind :return :value 2})]}))  # 5

(expect-facts (glean [f1]) 0
              {:next [;(steps 5) (exit 5)]
               :alloc [['a 0]]
               :use [['a 5]]
               :return [['a 5]]})

# The store of a into b at 3 is dead: the store at 5 comes before any load.
(def f2
  (function {:name 'f2
             :slots ['a 'b]
             :blocks [(basic-block 0
                                   [{:kind :make-array-mut
                                     :dst 0
                                     :elements []
                                     :region 3}  # 0
                                    {:kind :store-local :slot 0 :src 0}  # 1  a
                                    {:kind :load-local :dst 1 :slot 0}  # 2
                                    {:kind :store-local :slot 1 :src 1}  # 3  dead
                                    {:kind :make-array-mut
                                     :dst 2
                                     :elements []
                                     :region 4}  # 4
                                    {:kind :store-local :slot 1 :src 2}  # 5  b
                                    {:kind :load-local :dst 3 :slot 1}]  # 6
                                   {:kind :return :value 3})]}))  # 7

(expect-facts (glean [f2]) 0
              {:next [;(steps 7) (exit 7)]
               :alloc [['a 0] ['b 4]]
               :use [['b 7]]
               :return [['b 7]]})

# One capture, so environment index 2 is slot 1. The load at 4 reads slot 1,
# so the store at 3 is a pass.
(def f3
  (function {:name 'f3
             :captures ['k]
             :slots ['c 'd]
             :blocks [(basic-block 0
                                   [{:kind :make-array-mut
                                     :dst 0
                                     :elements []
                                     :region 3}  # 0
                                    {:kind :store-local :slot 0 :src 0}  # 1  c
                                    {:kind :load-local :dst 1 :slot 0}  # 2
                                    {:kind :store-local :slot 1 :src 1}  # 3  c to d
                                    {:kind :load-capture-raw :dst 2 :index 2}]  # 4  d
                                   {:kind :return :value 2})]}))  # 5

(expect-facts (glean [f3]) 0
              {:next [;(steps 5) (exit 5)]
               :free ['k]
               :captured ['k]
               :alloc [['c 0]]
               :pass [['c 'd 3]]
               :use [['d 5]]
               :return [['d 5]]})

# The StoreCapture at 4 stores into slot 1 before the load at 5, so the store
# at 3 is dead. It still fills slot 1, so the load names d.
(def f4
  (function {:name 'f4
             :captures ['k]
             :slots ['c 'd]
             :blocks [(basic-block 0
                                   [{:kind :make-array-mut
                                     :dst 0
                                     :elements []
                                     :region 3}  # 0
                                    {:kind :store-local :slot 0 :src 0}  # 1  c
                                    {:kind :load-local :dst 1 :slot 0}  # 2
                                    {:kind :store-local :slot 1 :src 1}  # 3  dead
                                    {:kind :store-capture :index 2 :src 1}  # 4
                                    {:kind :load-capture :dst 2 :index 2}]  # 5  d
                                   {:kind :return :value 2})]}))  # 6

(def stored (glean [f4]))
(expect-facts stored 0
              {:next [;(steps 6) (exit 6)]
               :free ['k]
               :captured ['k]
               :alloc [['c 0]]
               :use [['c 4] ['d 6]]
               :return [['d 6]]})
(assert (= (get (function-of stored 0) :marks) {:store-capture 1})
        "a StoreCapture is marked")

# The then arm loads r, so the store of a into r at 3 is a pass. The else arm
# makes a value whose one store, at 9, no load reads: it gives no fact, and
# still names the value r.
(def f5
  (function {:name 'f5
             :slots ['a 'r]
             :blocks [(basic-block 0
                                   [{:kind :make-array-mut
                                     :dst 0
                                     :elements []
                                     :region 3}  # 0
                                    {:kind :store-local :slot 0 :src 0}  # 1  a
                                    {:kind :load-local :dst 1 :slot 0}  # 2
                                    {:kind :store-local :slot 1 :src 1}  # 3  a to r
                                    {:kind :const :dst 2 :value true}]  # 4
                                   {:kind :branch
                                    :cond 2
                                    :then-label 1
                                    :else-label 2})  # 5
                      (basic-block 1
                                   [{:kind :load-local :dst 3 :slot 1}]  # 6  r
                                   {:kind :return :value 3})  # 7
                      (basic-block 2
                                   [{:kind :make-array-mut
                                     :dst 4
                                     :elements []
                                     :region 4}  # 8
                                    {:kind :store-local :slot 1 :src 4}]  # 9  dead
                                   {:kind :return :value 4})]}))  # 10

(expect-facts (glean [f5]) 0
              {:next [;(steps 5) [5 6] [5 8] [6 7] (exit 7) [8 9] [9 10]
                      (exit 10)]
               :alloc [['a 0] ['r 8]]
               :immediate [[:imm 4]]
               :pass [['a 'r 3]]
               :use [[[:imm 4] 5] ['r 7] ['r 10]]
               :return [['r 7] ['r 10]]})

(println "glean-dead: ok")
