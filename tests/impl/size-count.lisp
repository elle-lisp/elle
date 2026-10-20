(elle/epoch 15)
# audited: 2026-10-20
# The size producer's arithmetic: the bucket a source reads at, and the excess split between the work and the tables.
# tests/ratchet/overview.md
#
# The producer reads the real tree, whose tables list is empty, so a split
# that dropped a table's rows or counted them twice would read the same there
# as a correct one. These cases give the list a table.

(include-file "../ratchet/size/count.lisp")

(assert (nil? (reading 0)) "a source with no tokens reads nothing")
(assert (nil? (reading 2500)) "a source at the cap reads nothing")
(assert (= (reading 2501) 3000)
        "one token over the cap reads at the next bucket")
(assert (= (reading 3000) 3000) "a source on a bucket reads at that bucket")
(assert (= (reading 3001) 3500) "and one token past it at the next")

(def read [["a.rs" 3000] ["b.rs" 3500] ["w.rs" 4000]])

(let [split (excess read [{:path "w.rs" :rows "one row per fact"}])]
  (assert (= (get split :sheds) 1500)
          "the sources no table names owe their excess")
  (assert (= (get split :tables) 1500)
          "and the table's own is the second number"))

(let [split (excess read [])]
  (assert (= (get split :sheds) 3000) "with no table, every excess falls")
  (assert (= (get split :tables) 0) "and the tables carry nothing"))
