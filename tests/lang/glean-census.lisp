(elle/epoch 18)
# audited: 2026-11-23
# A census row compares a fence's hand facts with the gleaned facts of the unit's function of its name, judged alike.
# tools/glean/census.md
#
# The counter-factuals: a row that compared glean's symbols with the hand
# facts' keywords as they are would list a as released in each answer alone,
# and a judged row that ran no captures would list a as released by hand alone.

(def {:row row :judged-row judged-row} ((import "../../tools/glean/rows")))

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

# c2 of capture.md: a is used at 2 and k after it at 3, so the capture is
# late. The raw judge releases a after its use, and judged moves a into k and
# releases k alone. Glean's facts here hold no capture, and release k alone.
(def late
  {:name 'c2
   :facts {:next [[0 1] [1 2] [2 3] [3 4]]
           :alloc [[:a 0] [:k 1]]
           :capture [[:a :k 1]]
           :use [[:a 2] [:k 3]]}})
(def late-answer
  {:functions [{:id 0
                :name 'c2
                :marks {}
                :facts {:next [[0 1] [1 2] [2 3] [3 4]]
                        :alloc [['k 1]]
                        :use [['k 3]]}}]
   :closures []})

(assert (= {:document "d"
            :name 'c2
            :marks {}
            :released {:hand [:a] :glean []}
            :refused {:hand [] :glean []}} (row "d" late late-answer))
        "the raw judge releases a by hand alone")
(assert (= {:document "d"
            :name 'c2
            :marks {}
            :released {:hand [] :glean []}
            :refused {:hand [] :glean []}} (judged-row "d" late late-answer))
        "a judged row moves a into k, and the two answers agree")
(println "glean-census: ok")
