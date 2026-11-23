(elle/epoch 18)
# audited: 2026-11-23
# Glean raises :glean/noise at each fact the hand facts model otherwise, naming its cause, function, point and values.
# tools/glean/census.md
#
# The counter-factuals: a value made where the function still returns, a call
# whose :spec result is no :fresh, the resume value of a yield, and an
# immediate stored by @array are no noise. A glean that raised for them would
# let a cause claim a name it does not explain, and the census would hide that
# name.

(include-file "glean/helpers.lisp")
(def {:glean glean} ((import "../../tools/glean/glean")))

(defn invoke [dst func args]
  "A call instruction, as the lowerer writes a call it checked."
  {:kind :call
   :dst dst
   :func func
   :args args
   :arity-checked true
   :region 0
   :moves []})

# (defn a [] (def t <hoisted>) (def x (@array 1)) (if (length x) ... (error ...)) t)
(def a
  (function {:name 'a
             :captures [:synthetic]
             :slots ['t 'x]
             :blocks [(basic-block 0
                                   [{:kind :load-capture :dst 0 :index 0}  # 0  the literal
                                    {:kind :store-local :slot 0 :src 0}  # 1  t
                                    {:kind :const :dst 1 :value 1}  # 2
                                    {:kind :value-const :dst 2 :value @array}  # 3
                                    (invoke 3 2 [1])  # 4  x is made by a call
                                    {:kind :store-local :slot 1 :src 3}  # 5  x
                                    {:kind :value-const :dst 4 :value length}  # 6
                                    (invoke 5 4 [3])  # 7  length declares nothing
                                    {:kind :get :dst 6 :obj 3 :key 1}]  # 8  get has no rule
                                   {:kind :branch
                                    :cond 5
                                    :then-label 1
                                    :else-label 2})  # 9
                      (basic-block 1
                                   [{:kind :make-array-mut
                                     :dst 10
                                     :elements []
                                     :region 0}  # 10
                                    {:kind :load-local :dst 9 :slot 0}]  # 11
                                   {:kind :return :value 9})  # 12
                      (basic-block 2
                                   [{:kind :materialize-const
                                     :dst 7
                                     :template "boom"
                                     :region 0}]  # 13
                                   {:kind :emit
                                    :signal |:error|
                                    :value 7
                                    :resume-label 3})  # 14
                      (basic-block 3
                                   [{:kind :load-resume-value :dst 8}]  # 15
                                   {:kind :jump :target 1})]}))  # 16

# (defn b [s] (match s ...)): a MatchFail, then a yield, a call to fiber/value,
# and s stored into a new @array.
(def b
  (function {:id 1
             :name 'b
             :arity [1 1]
             :params 1
             :slots ['s]
             :blocks [(basic-block 0
                                   [{:kind :load-capture-raw :dst 0 :index 0}  # 0  s
                                    {:kind :store-local :slot 0 :src 0}  # 1
                                    {:kind :match-fail :dst 1 :src 0}]  # 2
                                   {:kind :emit
                                    :signal |:yield|
                                    :value 0
                                    :resume-label 1})  # 3
                      (basic-block 1
                                   [{:kind :load-resume-value :dst 2}  # 4
                                    {:kind :value-const
                                     :dst 3
                                     :value fiber/value}  # 5
                                    (invoke 4 3 [0])  # 6
                                    {:kind :value-const :dst 5 :value @array}  # 7
                                    (invoke 6 5 [0])]  # 8  s is stored
                                   {:kind :return :value 6})]}))  # 9

(def [answer noise] (heard (fn [] (glean [a b]))))

# The literal reaches t by a pass, so the census closes over it; x is the
# result of @array, whose :spec result is :fresh. The make-array-mut at 10 is
# made where a returns, so it is no noise.
(expect-facts answer 0
              {:next [;(steps 12) [9 13] (exit 12) [13 14] [14 15] [15 16]
                      [16 10]]
               :pass [[[:capture 0] 't 1]]
               :free [[:capture 0]]
               :captured [[:capture 0]]
               :immediate [[:imm 2]]
               :call [['x "@array" 4]]
               :arg [[[:imm 2] 0 4]]
               :alloc [[[:reg 10] 10] [[:reg 7] 13] [[:reg 8] 15]]
               :immutable [[:reg 7]]
               :cross [[[:reg 7] 14]]
               :use [[[:imm 2] 4] ['x 7] ['x 8] [[:imm 2] 8] [[:reg 5] 9]
                     ['t 12] [[:reg 7] 14]]
               :return [['t 12]]})

(assert (= (set ;noise)
           |{:cause :hoisted :id 0 :point nil :values [[:capture 0]]}
            {:cause :constructor :id 0 :point 4 :values ['x]}
            {:cause :unruled :id 0 :point 7 :values [[:reg 5] 'x]}
            {:cause :unruled :id 0 :point 8 :values [[:reg 6] 'x [:imm 2]]}
            {:cause :error-path :id 0 :point 13 :values [[:reg 7]]}
            {:cause :error-path :id 0 :point 15 :values [[:reg 8]]}
            {:cause :error-path :id 1 :point 2 :values [[:reg 1]]}
            {:cause :stored :id 1 :point 8 :values ['s]}
            {:cause :constructor :id 1 :point 8 :values [[:reg 6]]}|)
        "glean raises noise for each cause, and none for the counter-factuals")
(assert (= (length noise) 9) "each once")

(def u
  (function {:name 'u
             :blocks [(basic-block 0
                                   [{:kind :frobnicate :dst 0}]  # 0
                                   {:kind :return :value 0})]}))  # 1

(def [unheard-ok? unheard] (protect (heard (fn [] (glean [u])))))
(assert (not unheard-ok?) "an error passes the mediator")
(assert (= :glean-error (get unheard :error)) "as glean raised it")

(println "glean-noise: ok")
