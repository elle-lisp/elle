(elle/epoch 18)
# audited: 2026-11-23
# Each cause claims the mismatched names of a census row that it explains, and the tally counts them.
# tools/glean/census.md
#
# The counter-factuals: noise from another function of the unit, a hand call
# whose callee glean calls by a closure constant, and a judged row that
# stopped. A census that let any of them claim a name would hide the name,
# so w and v stay unclaimed, and c stays unclaimed when the judged row stops.

(def {:claims claims :tally tally} ((import "../../tools/glean/causes")))

(defn h []
  nil)

# Hand facts: f calls g at 3 with u, and h at 4 with v.
(def fence
  {:name 'f
   :doc "a claim"
   :source "(defn f [] nil)"
   :check (fn [] true)
   :facts {:call [[:n :g 3] [:m :h 4]] :arg [[:u 0 3] [:v 0 4]]}})

# Glean's facts: the hoisted literal passes into t, x is made by @array, and
# the closure constant 0 is h.
(def answer
  {:functions [{:id 0
                :name 'f
                :marks {}
                :facts {:pass [[[:capture 0] 't 3]]
                        :call [['x "@array" 2] [[:reg 9] [:fn 0] 5]]
                        :use [['u 1] ['v 1] ['w 1] ['c 1]]}}]
   :closures [h]})

(def unit [{:id 0 :name 'f :slots ['t 'x nil] :captures [:synthetic]}])

(def noise
  [{:cause :constructor :id 0 :point 2 :values ['x]}
   {:cause :unruled :id 0 :point 2 :values ['x]}
   {:cause :hoisted :id 0 :point nil :values [[:capture 0]]}
   {:cause :unruled :id 1 :point 0 :values ['w]}])

(def raw
  {:document "d"
   :name 'f
   :marks {}
   :released {:hand [:x :t :n :u :q :w :v] :glean [[:reg 7]]}
   :refused {:hand [:c] :glean []}})

(def judged (put raw :refused {:hand [] :glean []}))

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
   (claim :released :glean [:reg 7] |:unnamed|)
   (claim :refused :hand :c |:captures|)])

(def got (claims fence answer unit noise raw judged))
(assert (= (length got) (length expected)) "one claim for each name of the row")
(each i in (range (length expected))
  (assert (= (get got i) (get expected i))
          (string "claim " i " names the causes that explain it")))

(def stopped
  (claims fence answer unit noise raw {:document "d" :name 'f :error "boom"}))
(assert (= (get stopped 8) (claim :refused :hand :c ||))
        "a judged row that stopped claims nothing for the captures")
(assert (empty? (claims fence answer unit noise
                        {:document "d" :name 'f :error "boom"} judged))
        "a row that stopped holds no name to claim")

(assert (= (tally got)
           {:claimed {:constructor 1
                      :hoisted 1
                      :unruled 1
                      :error-path 0
                      :grafted 2
                      :unnamed 3
                      :captures 1}
            :alone {:constructor 0
                    :hoisted 1
                    :unruled 0
                    :error-path 0
                    :grafted 1
                    :unnamed 2
                    :captures 1}
            :shared 2
            :unclaimed [(get expected 5) (get expected 6)]})
        "the tally counts each cause's claims, those it makes alone, the shared, and the rest")

(println "glean-causes: ok")
