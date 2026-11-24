(elle/epoch 18)
# audited: 2026-11-24
# The table of arguments: the argument lists that drive each fence's function down its arms, and a size for the late witness.
# tools/glean/witness.md

(fn []
  (defn row [document name args &named sized]
    "The entry for the fence NAME of DOCUMENT: ARGS, the source text of each
  argument list, and SIZED, a source text function from a size to one."
    {:document document :name name :args args :sized sized})

  (def by-size "(fn [n] [n])")
  (def by-range "(fn [n] [(range n)])")

  [(row "again.md" "lb" ["[false]"]) (row "again.md" "walk" ["[]"])
   (row "capture.md" "lc" ["[[1 2 3]]"] :sized by-range)
   (row "capture.md" "l5" ["[[1 2 3]]"] :sized by-range)
   (row "closure.md" "ck" ["[false]" "[true]"])
   (row "group.md" "swap-wrap" ["[[@[1]] [@[2]] 0]" "[[@[1]] [@[2]] 3]"]
        :sized "(fn [n] [[@[1]] [@[2]] n])")
   (row "group.md" "walk-graft" ["[(list @[1] @[2]) ()]"])
   (row "group.md" "wg" ["[]"])
   (row "payload.md" "collect" ["[(fn [] (yield @[1]) (yield @[2]) nil)]"])
   (row "recur.md" "l1" ["[]"]) (row "recur.md" "l4" ["[]"])
   (row "recursion.md" "consume2" ["[@[1] 0]" "[@[1] 3]"]
        :sized "(fn [n] [@[1] n])")
   (row "recursion.md" "count-list" ["[(list 1 2 3) 0]"])
   (row "recursion.md" "build" ["[0]" "[3]"] :sized by-size)
   (row "recursion.md" "last-of" ["[(list @[1] @[2])]"])
   (row "release.md" "l" ["[[1 2 3]]"])
   (row "steps.md" "add-n" ["[@[] @[1] 0]" "[@[] @[1] 3]"]
        :sized "(fn [n] [@[] @[1] n])")
   (row "steps.md" "ins" ["[@[] @[1]]" "[@[@[]] @[1]]"])])
