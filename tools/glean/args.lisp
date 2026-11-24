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
  (def no-args ["[]"])
  (def either-way ["[true]" "[false]"])

  # Arguments alone drive no arm of anew.md's ms, ns and rs, whose loop never
  # ends once it starts; of a fence that yields outside a fiber (crossing.md,
  # payload.md's relay); or of a function a letrec binds with a body of nil
  # (mutual.md, recursion.md's od2). A loop that ends only on false runs once.
  [(row "again.md" "ga" ["[false]"]) (row "again.md" "lb" ["[false]"])
   (row "again.md" "walk" no-args) (row "aliased.md" "a1" no-args)
   (row "aliased.md" "fwd" ["[@[1] @[2]]"])
   (row "borrows.md" "keep-first" ["[[@[1]]]" "[(list @[1])]"])
   (row "borrows.md" "kc" no-args) (row "borrows.md" "kh" no-args)
   (row "call.md" "m" no-args) (row "call.md" "n" ["[[@[1]]]"])
   (row "call.md" "reset" ["[@{}]"]) (row "capture.md" "c3" no-args)
   (row "capture.md" "lc" ["[[1 2 3]]"] :sized by-range)
   (row "capture.md" "l5" ["[[1 2 3]]"] :sized by-range)
   (row "closure.md" "bb" either-way) (row "closure.md" "bh" either-way)
   (row "closure.md" "ck" either-way) (row "datalog.md" "f" ["[[@[1]] @[]]"])
   (row "group.md" "swap-wrap" ["[[@[1]] [@[2]] 0]" "[[@[1]] [@[2]] 3]"]
        :sized "(fn [n] [[@[1]] [@[2]] n])")
   (row "group.md" "walk-graft" ["[(list @[1] @[2]) ()]"])
   (row "group.md" "wg" no-args) (row "handover.md" "kw" either-way)
   (row "handover.md" "due" either-way) (row "handover.md" "kv" either-way)
   (row "handover.md" "lv" ["[false]"]) (row "itself.md" "randomize" ["[@[0]]"])
   (row "itself.md" "keep-it" ["[@[] @[1]]"])
   (row "itself.md" "kept-r" ["[@[] @[1] true]" "[@[] @[1] false]"])
   (row "itself.md" "maybe" ["[@[1] true]" "[@[1] false]"])
   (row "liveness.md" "cu" no-args) (row "liveness.md" "p" no-args)
   (row "liveness.md" "hw" either-way) (row "liveness.md" "lx" ["[false]"])
   (row "owner.md" "o1" no-args) (row "owner.md" "o2" no-args)
   (row "owner.md" "o3" no-args) (row "owner.md" "o6" no-args)
   (row "payload.md" "cap" ["[(fn [] (yield @[1]) nil)]"])
   (row "payload.md" "collect" ["[(fn [] (yield @[1]) (yield @[2]) nil)]"])
   (row "payload.md" "pk"
        ["[(fiber/new (fn [] (yield @[1]) nil) |:yield|) @[]]"])
   (row "place.md" "split" ["[@[] @[] @[1] true]" "[@[] @[] @[1] false]"])
   (row "place.md" "into-either" ["[@[] @[] @[1] true]" "[@[] @[] @[1] false]"])
   (row "recur.md" "l1" no-args) (row "recur.md" "l2" ["[false]"])
   (row "recur.md" "l3" ["[false]"]) (row "recur.md" "l4" no-args)
   (row "recur.md" "bk" either-way)
   (row "recursion.md" "consume2" ["[@[1] 0]" "[@[1] 3]"]
        :sized "(fn [n] [@[1] n])")
   (row "recursion.md" "count-list" ["[(list 1 2 3) 0]"])
   (row "recursion.md" "build" ["[0]" "[3]"] :sized by-size)
   (row "recursion.md" "last-of" ["[(list @[1] @[2])]"])
   (row "release.md" "g" ["[1]" "[0]"]) (row "release.md" "j" ["[1]" "[0]"])
   (row "release.md" "l" ["[[1 2 3]]"]) (row "removal.md" "pr" no-args)
   (row "return.md" "either" ["[@[1] @[2] true]" "[@[1] @[2] false]"])
   (row "return.md" "caller" ["[[@[1]]]"]) (row "sink.md" "s2" no-args)
   (row "sink.md" "s3" no-args) (row "sink.md" "sk" no-args)
   (row "states.md" "fresh-or" ["[@[1] true]" "[@[1] false]"])
   (row "steps.md" "add-n" ["[@[] @[1] 0]" "[@[] @[1] 3]"]
        :sized "(fn [n] [@[] @[1] n])")
   (row "steps.md" "ins" ["[@[] @[1]]" "[@[@[]] @[1]]"])
   (row "stored.md" "keep" ["[@[] @[1]]"])
   (row "stored.md" "via" ["[@[] @[1]]"])
   (row "stored.md" "wrap-if" ["[@[1] true]" "[@[1] false]"])
   (row "stored.md" "maybe-keep" ["[@[] @[1] true]" "[@[] @[1] false]"])
   (row "stored.md" "wrap-or" ["[@[1] true]" "[@[1] false]"])
   (row "stored.md" "wrap-both" ["[@[1] true]" "[@[1] false]"])
   (row "through.md" "kb" no-args) (row "through.md" "wrap2" no-args)
   (row "unused.md" "up" either-way)
   (row "unused.md" "ub" ["[[@[1] @[2]] true]" "[[@[1] @[2]] false]"])])
