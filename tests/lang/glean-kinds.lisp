(elle/epoch 18)
# audited: 2026-11-23
# Glean gives each ruled kind its facts: what it makes, from what, what it reads out of a container, and what it crosses.
# tools/glean/overview.md
#
# The counter-factual: the empty list is an immediate, so its operand and
# capture facts name [:imm 2], and immediate([:imm 2]) holds. A glean that named
# it nothing would drop both facts, and one that gave no immediate fact would
# leave the rules refusing a store of it for want of an owner.

(include-file "glean/helpers.lisp")
(def glean (quiet (get ((import "../../tools/glean/glean")) :glean)))

# A pair of x and the empty list, a string literal, and a closure built over
# x, s and the empty list.
(def k
  (function {:name 'k
             :arity [1 1]
             :params 1
             :slots ['x 'p 's 'c]
             :blocks [(basic-block 0
                                   [{:kind :load-capture-raw :dst 0 :index 0}  # 0  x
                                    {:kind :store-local :slot 0 :src 0}  # 1
                                    {:kind :const :dst 1 :value ()}  # 2
                                    {:kind :list
                                     :dst 2
                                     :head 0
                                     :tail 1
                                     :region 5}  # 3
                                    {:kind :store-local :slot 1 :src 2}  # 4  p
                                    {:kind :materialize-const
                                     :dst 3
                                     :template "hi"
                                     :region 6}  # 5
                                    {:kind :store-local :slot 2 :src 3}  # 6  s
                                    {:kind :make-closure
                                     :dst 4
                                     :closure-id 1
                                     :captures [0 3 1]
                                     :region 7}  # 7
                                    {:kind :store-local :slot 3 :src 4}]  # 8  c
                                   {:kind :return :value 4})]}))  # 9

(expect-facts (glean [k]) 0
              {:next [;(steps 9) (exit 9)]
               :param [['x 0]]
               :alloc [['p 3] ['s 5] ['c 7]]
               :immediate [[:imm 2]]
               :operand [['x 'p 3] [[:imm 2] 'p 3]]
               :immutable ['p 's]
               :capture [['x 'c 7] ['s 'c 7] [[:imm 2] 'c 7]]
               :use [['x 3] [[:imm 2] 3] ['x 7] ['s 7] [[:imm 2] 7] ['c 9]]
               :return [['c 9]]})

# Each kind that reads a value out of a container, each read out of col.
(def r
  (function {:name 'r
             :arity [1 1]
             :params 1
             :slots ['col]
             :blocks [(basic-block 0
                                   [{:kind :load-capture-raw :dst 0 :index 0}  # 0  col
                                    {:kind :store-local :slot 0 :src 0}  # 1
                                    {:kind :first :dst 1 :pair 0}  # 2
                                    {:kind :rest :dst 2 :pair 0}  # 3
                                    {:kind :first-or-nil :dst 3 :src 0}  # 4
                                    {:kind :rest-or-nil :dst 4 :src 0}  # 5
                                    {:kind :first-destructure :dst 5 :src 0}  # 6
                                    {:kind :rest-destructure :dst 6 :src 0}  # 7
                                    {:kind :array-mut-ref-destructure
                                     :dst 7
                                     :src 0
                                     :index 1}  # 8
                                    {:kind :array-mut-ref-or-nil
                                     :dst 8
                                     :src 0
                                     :index 2}  # 9
                                    {:kind :struct-get-or-nil
                                     :dst 9
                                     :src 0
                                     :key :a}  # 10
                                    {:kind :struct-get-destructure
                                     :dst 10
                                     :src 0
                                     :key :b}]  # 11
                                   {:kind :return :value 1})]}))  # 12

(def reads (->array (map (fn [i] [[:reg i] 'col (+ i 1)]) (range 1 11))))
(expect-facts (glean [r]) 0
              {:next [;(steps 12) (exit 12)]
               :param [['col 0]]
               :read reads
               :use [;(->array (map (fn [i] ['col (+ i 1)]) (range 1 11)))
                     [[:reg 1] 12]]
               :return [[[:reg 1] 12]]})

# An Eval with no environment makes its result: its environment register holds
# the immediate nil. A failed match crosses its scrutinee and makes the resume
# value. A bound check uses alone, and a length is a computed immediate.
(def m
  (function {:name 'm
             :arity [1 1]
             :params 1
             :slots ['d]
             :blocks [(basic-block 0
                                   [{:kind :load-capture-raw :dst 0 :index 0}  # 0  d
                                    {:kind :store-local :slot 0 :src 0}  # 1
                                    {:kind :const :dst 1 :value nil}  # 2
                                    {:kind :eval :dst 2 :expr 0 :env 1}  # 3
                                    {:kind :check-signal-bound
                                     :src 2
                                     :allowed-bits |:error|}  # 4
                                    {:kind :length :dst 3 :src 0}  # 5
                                    {:kind :match-fail :dst 4 :src 0}]  # 6
                                   {:kind :return :value 3})]}))  # 7

(expect-facts (glean [m]) 0
              {:next [;(steps 7) (exit 7)]
               :param [['d 0]]
               :alloc [[[:reg 2] 3] [[:reg 4] 6]]
               :immediate [[:imm 2] [:imm 5]]
               :cross [['d 6]]
               :use [['d 3] [[:imm 2] 3] [[:reg 2] 4] ['d 5] ['d 6] [[:imm 5] 7]]
               :return [[[:imm 5] 7]]})
(assert (empty? (get (function-of (glean [m]) 0) :marks))
        "an Eval with no environment is ruled")

(println "glean-kinds: ok")
