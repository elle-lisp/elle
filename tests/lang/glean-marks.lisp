(elle/epoch 18)
# audited: 2026-11-22
# Glean marks a function with each unruled kind it holds and its count, and an unruled instruction gives next and use alone.
# tools/glean/overview.md
#
# The counter-factual: an Eval that passes an environment may store its datum
# anywhere the environment reaches, so a glean that ruled it as the Eval with
# none would claim the result is fresh and the datum only used.

(include-file "glean/helpers.lisp")
(def {:glean glean} ((import "../../tools/glean/glean")))

# (defn n [d env] ...): an Eval with an environment, and a capture cell made
# from its result and written twice.
(def n
  (function {:name 'n
             :arity [2 2]
             :params 2
             :slots ['d 'env]
             :blocks [(basic-block 0
                                   [{:kind :load-capture-raw :dst 0 :index 0}  # 0  d
                                    {:kind :store-local :slot 0 :src 0}  # 1
                                    {:kind :load-capture-raw :dst 1 :index 1}  # 2  env
                                    {:kind :store-local :slot 1 :src 1}  # 3
                                    {:kind :eval :dst 2 :expr 0 :env 1}  # 4
                                    {:kind :make-capture-cell
                                     :dst 3
                                     :value 2
                                     :region 3
                                     :name 'cell
                                     :mutated true}  # 5
                                    {:kind :store-capture-cell :cell 3 :value 0}  # 6
                                    {:kind :store-capture-cell :cell 3 :value 1}]  # 7
                                   {:kind :return :value 3})]}))  # 8

(def answer (glean [n]))
(expect-facts answer 0
              {:next [;(steps 8) (exit 8)]
               :param [['d 0] ['env 1]]
               :use [['d 4] ['env 4] [[:reg 2] 5] [[:reg 3] 6] ['d 6]
                     [[:reg 3] 7] ['env 7] [[:reg 3] 8]]
               :return [[[:reg 3] 8]]})
(assert (= (get (function-of answer 0) :marks)
           {:eval 1 :make-capture-cell 1 :store-capture-cell 2})
        "each unruled kind marks the function with its count")

(println "glean-marks: ok")
