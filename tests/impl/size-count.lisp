(elle/epoch 15)
# audited: 2026-10-20
# The size producer's arithmetic: the cap of a file's kind, the bucket it reads at, and the excess split between the work and the tables.
# tests/ratchet/overview.md
#
# The producer reads the real tree, whose tables list is empty, so a split
# that dropped a table's rows or counted them twice would read the same there
# as a correct one. These cases give the list a table. A document between the
# two caps is the counter-factual for one cap over both kinds.

(include-file "../ratchet/size/count.lisp")

(assert (nil? (reading "a.rs" 0)) "a source with no tokens reads nothing")
(assert (nil? (reading "a.rs" 2500)) "a source at its cap reads nothing")
(assert (= (reading "a.rs" 2501) 3000)
        "one token over the cap reads at the next bucket")
(assert (= (reading "a.rs" 3000) 3000)
        "a source on a bucket reads at that bucket")
(assert (= (reading "a.rs" 3001) 3500) "and one token past it at the next")
(assert (= (reading "lib/a.lisp" 2501) 3000) "a Lisp source has the same cap")

(assert (nil? (reading "docs/a.md" 3500))
        "a document over the source cap and under its own reads nothing")
(assert (nil? (reading "docs/a.md" 4000)) "a document at its cap reads nothing")
(assert (= (reading "docs/a.md" 4001) 4500)
        "one token over a document's cap reads at the next bucket")

(def read [["a.rs" 3000] ["d.md" 4500] ["w.rs" 4000]])

(let [split (excess read [{:path "w.rs" :rows "one row per fact"}])]
  (assert (= (get split :sheds) 1000)
          "the files no table names owe their excess, each over its own cap")
  (assert (= (get split :tables) 1500)
          "and the table's own is the second number"))

(let [split (excess read [])]
  (assert (= (get split :sheds) 2500) "with no table, every excess falls")
  (assert (= (get split :tables) 0) "and the tables carry nothing"))
