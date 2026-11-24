(elle/epoch 18)
# audited: 2026-11-24
# The release diff reads a value early, late or none against the liveness the spec derives from glean's facts.
# tools/glean/diff.md
#
# The counter-factual: a diff that judged the steps that only move a value
# would read a late at 4 in d1, and one that judged a count instruction would
# read it late at 5. One that read no ended region would miss d3's early use.
# One that judged a value present did not count at its birth would read the
# retained read late and held in d5. One that read live in place of live-as
# would read a late at the jump in d6, where a block parameter carries it. One
# that made no late capture a store would read k late at 11 in d7.

(include-file "glean/helpers.lisp")
(def diff (get ((import "../../tools/glean/diff")) :diff))

(defn using [dst src]
  "A Length of the register SRC into DST: a step that uses SRC and makes no
   region."
  {:kind :length :dst dst :src src})

# a is released by the load and the count step after its last use at 3.
(def d1
  (function {:slots ['a]
             :blocks [(basic-block 0
                                   [(fresh-array 0 3)  # 0  a
                                    (store-slot 0 0)  # 1
                                    (load-slot 1 0)  # 2
                                    (using 2 1)  # 3  a is used
                                    (load-slot 3 0)  # 4
                                    (release-value 3)  # 5
                                    (nil-const 4)]  # 6
                                   (return-of 4))]}))  # 7
(expect-diff (diff [d1]) 0 {})

# b is made at 4 after the last use of a, and a is still held there.
(def d2
  (function {:slots ['a 'b]
             :blocks [(basic-block 0
                                   [(fresh-array 0 3)  # 0  a
                                    (store-slot 0 0)  # 1
                                    (load-slot 1 0)  # 2
                                    (using 2 1)  # 3  a is used
                                    (fresh-array 3 4)  # 4  b
                                    (store-slot 1 3)  # 5
                                    (load-slot 4 0)  # 6
                                    (release-value 4)  # 7
                                    (load-slot 5 1)  # 8
                                    (release-value 5)  # 9
                                    (nil-const 6)]  # 10
                                   (return-of 6))]}))  # 11
(expect-diff (diff [d2]) 0 {:late [['a 4]]})

# a is released at 3, before its use at 5.
(def d3
  (function {:slots ['a]
             :blocks [(basic-block 0
                                   [(fresh-array 0 3)  # 0  a
                                    (store-slot 0 0)  # 1
                                    (load-slot 1 0)  # 2
                                    (release-value 1)  # 3
                                    (load-slot 2 0)  # 4
                                    (using 3 2)  # 5  a is used
                                    (nil-const 4)]  # 6
                                   (return-of 4))]}))  # 7
(expect-diff (diff [d3]) 0 {:early [['a 5]]})

# a is never released, so it is held at the Return after its last use, and at
# the exit.
(def d4
  (function {:slots ['a]
             :blocks [(basic-block 0
                                   [(fresh-array 0 3)  # 0  a
                                    (store-slot 0 0)  # 1
                                    (load-slot 1 0)  # 2
                                    (using 2 1)  # 3  a is used
                                    (nil-const 3)]  # 4
                                   (return-of 3))]}))  # 5
(expect-diff (diff [d4]) 0 {:late [['a 5]] :none [['a 5]]})

# The read [:reg 1] begins with no count, so its retain at 2 ends a borrow and
# the diff judges it nowhere. The parameter x is live until its Return.
(def d5
  (function {:params 1
             :arity [1 1]
             :captures ['k]
             :slots ['x]
             :blocks [(basic-block 0
                                   [{:kind :load-capture :dst 0 :index 0}  # 0  k
                                    {:kind :first :dst 1 :pair 0}  # 1  [:reg 1]
                                    {:kind :incref-value-region :src 1}  # 2
                                    (load-slot 2 0)]  # 3  x
                                   (return-of 2))]}))  # 4
(expect-diff (diff [d5]) 0 {})

# The store at 3 hands a to the block parameter [:slot 1], whose use at 6
# keeps a live through the jump.
(def d6
  (function {:slots ['a nil]
             :blocks [(basic-block 0
                                   [(fresh-array 0 3)  # 0  a
                                    (store-slot 0 0)  # 1
                                    (load-slot 1 0)  # 2
                                    (store-slot 1 1)]  # 3  a is handed to [:slot 1]
                                   (jump-to 1))  # 4
                      (basic-block 1
                                   [(load-slot 2 1)  # 5
                                    (using 3 2)  # 6  [:slot 1] is used
                                    (load-slot 4 1)  # 7
                                    (release-value 4)  # 8
                                    (nil-const 5)]  # 9
                                   (return-of 5))]}))  # 10
(expect-diff (diff [d6]) 0 {})

# The then arm uses k after the last use of a there, so the capture is late,
# and k owns a. The else arm uses a alone, and k waits for it there. Both are
# held to the join, so each is late at the jumps after its last use, and a at
# the use of k.
(def d7
  (function {:slots ['a 'k]
             :blocks [(basic-block 0
                                   [(fresh-array 0 3)  # 0  a
                                    (store-slot 0 0)  # 1
                                    (load-slot 1 0)  # 2
                                    {:kind :make-closure
                                     :dst 2
                                     :closure-id 1
                                     :captures [1]
                                     :region 4}  # 3  k captures a
                                    (store-slot 1 2)  # 4
                                    {:kind :const :dst 3 :value true}]  # 5
                                   (branch-on 3 1 2))  # 6
                      (basic-block 1
                                   [(load-slot 4 1)  # 7
                                    (using 5 4)]  # 8  k is used
                                   (jump-to 3))  # 9
                      (basic-block 2
                                   [(load-slot 6 0)  # 10
                                    (using 7 6)]  # 11  a is used
                                   (jump-to 3))  # 12
                      (basic-block 3
                                   [(load-slot 8 0)  # 13
                                    (release-value 8)  # 14
                                    (load-slot 9 1)  # 15
                                    (release-value 9)  # 16
                                    (nil-const 10)]  # 17
                                   (return-of 10))]}))  # 18
(expect-diff (diff [d7]) 0 {:late [['a 8] ['a 9] ['a 12] ['k 9] ['k 12]]})

(println "glean-diff: ok")
