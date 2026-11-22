(elle/epoch 18)
# audited: 2026-11-22
# A census row compares a fence's hand facts with the gleaned facts of the unit's function of its name, judged alike.
# tools/glean/overview.md
#
# The counter-factual: a row that compared glean's symbols with the hand
# facts' keywords as they are would list a as released in each answer alone.

(def {:row row} ((import "../../tools/glean/rows")))

# Hand facts: a is made at 0 and used at 1, so released-all releases it on the
# edge into the exit. x is read out of coll and stored into acc, so refusals
# refuses that store.
(def fence
  {:name 'f
   :doc "a claim"
   :source "(defn f [] nil)"
   :check (fn [] true)
   :facts {:next [[0 1] [1 2]]
           :alloc [[:a 0]]
           :read [[:x :coll 0]]
           :store [[:x :acc 1]]
           :use [[:a 1]]}})

# Gleaned facts: a is made at 0 and used at 2; [:reg 4] is made at 1 and never
# used, so it is released at once. Nothing is stored, so nothing is refused.
(def answer
  {:functions [{:id nil :name nil :facts {} :marks {}}
               {:id 0
                :name 'f
                :marks {:get 2}
                :facts {:next [[0 1] [1 2] [2 3]]
                        :alloc [['a 0] [[:reg 4] 1]]
                        :use [['a 2]]}}]
   :closures []})

(assert (= {:document "d"
            :name 'f
            :marks {:get 2}
            :released {:hand [] :glean [[:reg 4]]}
            :refused {:hand [:x] :glean []}} (row "d" fence answer))
        "a row lists the names each answer holds alone, a glean symbol as its keyword")
(assert (= {:document "d" :name 'g :error "no function is named g"}
           (row "d" (put fence :name 'g) answer))
        "a fence whose name no function bears gives a row with an error")
(println "glean-census: ok")
