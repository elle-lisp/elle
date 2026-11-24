(elle/epoch 18)
# audited: 2026-11-24
# Glean numbers each instruction and terminator in view order, and links each to what follows it, an exit included.
# tools/glean/overview.md
#
# The counter-factual: a TailCall to a closure replaces the frame, so a glean
# that gave it only its fall-through edge would keep the value it returns live
# past a point the frame never reaches.

(include-file "glean/helpers.lisp")
(def glean (quiet (get ((import "../../tools/glean/glean")) :glean)))

# The blocks in view order are 0, 2, 1, 3. A block with no instruction starts
# at its terminator, a count instruction gives no fact, an emit crosses its
# value and resumes at the block that loads the resume value, and an
# Unreachable leads nowhere.
(def e
  (function {:name 'e
             :blocks [(basic-block 0
                                   [{:kind :const :dst 0 :value true}]  # 0
                                   {:kind :branch
                                    :cond 0
                                    :then-label 1
                                    :else-label 2})  # 1
                      (basic-block 2
                                   [{:kind :decref-region :region-id 4}  # 2
                                    {:kind :make-array-mut
                                     :dst 1
                                     :elements []
                                     :region 4}]  # 3
                                   {:kind :emit
                                    :signal |:yield|
                                    :value 1
                                    :resume-label 3})  # 4
                      (basic-block 1 [] {:kind :unreachable})  # 5
                      (basic-block 3
                                   [{:kind :load-resume-value :dst 2}  # 6
                                    {:kind :decref-value-region :src 2}]  # 7
                                   {:kind :return :value 2})]}))  # 8

(expect-facts (glean [e]) 0
              {:next [[0 1] [1 5] [1 2] [2 3] [3 4] [4 6] [6 7] [7 8] (exit 8)]
               :alloc [[[:reg 1] 3] [[:reg 2] 6]]
               :immediate [[:imm 0]]
               :cross [[[:reg 1] 4]]
               :use [[[:imm 0] 1] [[:reg 1] 4] [[:reg 2] 8]]
               :return [[[:reg 2] 8]]})

# (defn t [g] (g)): the TailCall returns at its own point, and falls through
# for a native callee, whose result the Return after it returns.
(def t
  (function {:name 't
             :arity [1 1]
             :params 1
             :slots ['g nil]
             :blocks [(basic-block 0
                                   [{:kind :load-capture-raw :dst 0 :index 0}  # 0  g
                                    {:kind :store-local :slot 0 :src 0}  # 1
                                    {:kind :load-local :dst 1 :slot 0}  # 2
                                    {:kind :tail-call
                                     :dst 2
                                     :func 1
                                     :args []
                                     :arity-checked false
                                     :region 3
                                     :defer-callee-release false
                                     :deferred-release-slots []
                                     :borrowed-arg-slots []
                                     :late-mints []}  # 3
                                    {:kind :store-local :slot 1 :src 2}  # 4
                                    {:kind :load-local :dst 3 :slot 1}]  # 5
                                   {:kind :return :value 3})]}))  # 6

(expect-facts (glean [t]) 0
              {:next [;(steps 6) (exit 3) (exit 6)]
               :param [['g 0]]
               :call [[[:slot 1] 'g 3]]
               :use [['g 3] [[:slot 1] 6]]
               :return [[[:slot 1] 3] [[:slot 1] 6]]})

(println "glean-points: ok")
