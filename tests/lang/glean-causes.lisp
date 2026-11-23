(elle/epoch 18)
# audited: 2026-11-23
# Each cause claims the mismatched names of a census row that it explains, and the tally counts them.
# tools/glean/census.md
#
# The counter-factuals: noise from another function of the unit, a hand call
# whose callee glean calls by a closure constant, a judged row that stopped,
# a value made on a line of f's own, and an alloc whose glean call is to a
# native. A census that let any of them claim a name would hide the name, so
# w, v and x keep the causes below, and c stays unclaimed when the judged row
# stops.

(def {:claims claims :tally tally} ((import "../../tools/glean/causes")))

(defn h []
  nil)

# Hand facts: f makes x at 0 and k at 1, and calls g at 3 with u, and h at 4
# with v.
(def fence
  {:name 'f
   :doc "a claim"
   :source "(defn f [] nil)"
   :check (fn [] true)
   :facts {:alloc [[:x 0] [:k 1]]
           :call [[:n :g 3] [:m :h 4]]
           :arg [[:u 0 3] [:v 0 4]]}})

# Glean's facts: the hoisted literal passes into t, x is made by @array, k by
# a call of the closure make, the closure constant 0 is h, [:reg 7] is made on
# f's line 1 and [:reg 8] on line 9, g's line, and [:reg 8] passes into z.
(def answer
  {:functions [{:id 0
                :name 'f
                :marks {}
                :facts {:pass [[[:capture 0] 't 3] [[:reg 8] 'z 2]]
                        :alloc [[[:reg 7] 0] [[:reg 8] 1]]
                        :call [['x "@array" 2] [[:reg 9] [:fn 0] 5] ['k 'make 5]]
                        :use [['u 1] ['v 1] ['w 1] ['c 1] ['y 1]]}}]
   :closures [h]})

(def unit
  [{:id 0
    :name 'f
    :slots ['t 'x nil]
    :captures [:synthetic]
    :blocks [{:label 0
              :instrs [{:kind :const :dst 7 :value nil :at [1 1]}  # 0
                       {:kind :const :dst 8 :value nil :at [9 4]}]  # 1
              :term {:kind :return :value 8}}]}  # 2
   {:id 1
    :name 'g
    :slots []
    :captures []
    :blocks [{:label 0
              :instrs [{:kind :const :dst 0 :value nil :at [9 1]}]  # 0
              :term {:kind :return :value 0}}]}])  # 1

(def noise
  [{:cause :constructor :id 0 :point 2 :values ['x]}
   {:cause :unruled :id 0 :point 2 :values ['x]}
   {:cause :hoisted :id 0 :point nil :values [[:capture 0]]}
   {:cause :stored :id 0 :point 2 :values ['y]}
   {:cause :unruled :id 1 :point 0 :values ['w]}])

(def raw
  {:document "d"
   :name 'f
   :marks {}
   :released {:hand [:x :t :n :u :q :w :v :k] :glean [[:reg 7] [:reg 8] :z]}
   :refused {:hand [:c :y] :glean []}})

(def judged (put raw :refused {:hand [:y] :glean []}))

(defn claim [judgment side value causes]
  "The claim on VALUE, on SIDE of JUDGMENT in f's row, by CAUSES."
  {:document "d"
   :name 'f
   :judgment judgment
   :side side
   :value value
   :causes causes})

(def expected
  [(claim :released :hand :x |:constructor :unruled|)
   (claim :released :hand :t |:hoisted|)
   (claim :released :hand :n |:grafted :unnamed|)
   (claim :released :hand :u |:grafted|) (claim :released :hand :q |:unnamed|)
   (claim :released :hand :w ||) (claim :released :hand :v ||)
   (claim :released :hand :k |:closure-result|)
   (claim :released :glean [:reg 7] |:unnamed|)
   (claim :released :glean [:reg 8] |:grafted :unnamed|)
   (claim :released :glean :z |:grafted|) (claim :refused :hand :c |:captures|)
   (claim :refused :hand :y |:stored|)])

(def got (claims fence answer unit noise raw judged))
(assert (= (length got) (length expected)) "one claim for each name of the row")
(each i in (range (length expected))
  (assert (= (get got i) (get expected i))
          (string "claim " i " names the causes that explain it")))

(def stopped
  (claims fence answer unit noise raw {:document "d" :name 'f :error "boom"}))
(assert (= (get stopped 11) (claim :refused :hand :c ||))
        "a judged row that stopped claims nothing for the captures")
(assert (empty? (claims fence answer unit noise
                        {:document "d" :name 'f :error "boom"} judged))
        "a row that stopped holds no name to claim")

(assert (= (tally got)
           {:claimed {:constructor 1
                      :hoisted 1
                      :unruled 1
                      :error-path 0
                      :stored 1
                      :grafted 4
                      :unnamed 4
                      :captures 1
                      :closure-result 1}
            :alone {:constructor 0
                    :hoisted 1
                    :unruled 0
                    :error-path 0
                    :stored 1
                    :grafted 2
                    :unnamed 2
                    :captures 1
                    :closure-result 1}
            :shared 3
            :unclaimed [(get expected 5) (get expected 6)]})
        "the tally counts each cause's claims, those it makes alone, the shared, and the rest")

(println "glean-causes: ok")
