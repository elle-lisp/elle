(elle/epoch 18)
# audited: 2026-11-24
# An owned region is released with its owner, by a group free, or at the exit of the activation that adopted it.
# tools/glean/present.md
#
# The counter-factual: a reader that counted an owned region would release c
# at its own release in o1, before its owner, and would never release g. One
# that ignored the activation would hold a at o2's exit, and one that ignored
# a group free would release b at its later release instead of finding it
# over. One that read a cell's release by value as the cell's own would
# release cell in o3 at 6 and find its release at 8 over, and one that left
# the route slot filled in o4 would find a's later release over.

(include-file "glean/helpers.lisp")
(def present (get ((import "../../tools/glean/present")) :present))

# p owns c, and c owns g. A release of c does nothing, and the release of p
# releases all three.
(def o1
  (function {:slots ['p 'c 'g]
             :blocks [(basic-block 0
                                   [(fresh-array 0 3)  # 0  p
                                    (store-slot 0 0)  # 1
                                    (fresh-array 1 4)  # 2  c
                                    (store-slot 1 1)  # 3
                                    (fresh-array 2 5)  # 4  g
                                    (store-slot 2 2)  # 5
                                    (load-slot 3 0)  # 6
                                    (load-slot 4 1)  # 7
                                    {:kind :adopt-region :parent 3 :child 4}  # 8
                                    (load-slot 5 1)  # 9
                                    (load-slot 6 2)  # 10
                                    {:kind :adopt-region :parent 5 :child 6}  # 11
                                    (load-slot 7 1)  # 12
                                    (release-value 7)  # 13
                                    (load-slot 8 0)  # 14
                                    (release-value 8)  # 15
                                    (nil-const 9)]  # 16
                                   (return-of 9))]}))  # 17
(expect-present (present [o1]) 0 {:released [['p 15] ['c 15] ['g 15]]})

# The activation owns a, so its release does nothing and the exit releases it.
# A group free releases b and c whatever their counts, so b's later release is
# over.
(def o2
  (function {:slots ['a 'b 'c]
             :blocks [(basic-block 0
                                   [(fresh-array 0 3)  # 0  a
                                    (store-slot 0 0)  # 1
                                    (fresh-array 1 4)  # 2  b
                                    (store-slot 1 1)  # 3
                                    (fresh-array 2 5)  # 4  c
                                    (store-slot 2 2)  # 5
                                    (load-slot 3 0)  # 6
                                    {:kind :adopt-into-activation :child 3}  # 7
                                    (load-slot 4 0)  # 8
                                    (release-value 4)  # 9
                                    (load-slot 5 1)  # 10
                                    (load-slot 6 2)  # 11
                                    {:kind :free-region-group :members [5 6]}  # 12
                                    (load-slot 7 1)  # 13
                                    (release-value 7)  # 14
                                    (nil-const 8)]  # 15
                                   (return-of 8))]}))  # 16
(expect-present (present [o2]) 0
                {:released [['a 16] ['b 12] ['c 12]] :over [['b 14]]})

# A release by value of a cell releases its content, and a DecrefCellRegion
# releases the cell.
(def o3
  (function {:slots ['a 'cell]
             :blocks [(basic-block 0
                                   [(fresh-array 0 3)  # 0  a
                                    (store-slot 0 0)  # 1
                                    (load-slot 1 0)  # 2
                                    {:kind :make-capture-cell
                                     :dst 2
                                     :value 1
                                     :region 4
                                     :name 'a
                                     :mutated false}  # 3  cell
                                    (store-slot 1 2)  # 4
                                    (load-slot 3 1)  # 5
                                    (release-value 3)  # 6
                                    (load-slot 4 1)  # 7
                                    {:kind :decref-cell-region :src 4}  # 8
                                    (nil-const 5)]  # 9
                                   (return-of 5))]}))  # 10
(expect-present (present [o3]) 0 {:released [['a 6] ['cell 8]]})

# A postponed release lowers the count of the region its slot holds, and
# leaves the route slot empty, so the release behind it does nothing.
(def o4
  (function {:slots ['a nil]
             :blocks [(basic-block 0
                                   [(fresh-array 0 3)  # 0  a
                                    (store-slot 0 0)  # 1
                                    (load-slot 1 0)  # 2
                                    (store-slot 1 1)  # 3
                                    {:kind :decref-postponed :slot 1 :route 0}  # 4
                                    (load-slot 2 0)  # 5
                                    (release-value 2)  # 6
                                    (nil-const 3)]  # 7
                                   (return-of 3))]}))  # 8
(expect-present (present [o4]) 0 {:released [['a 4]]})

(println "glean-present-owners: ok")
