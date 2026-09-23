(elle/epoch 12)
# audited: 2026-09-23
# A process that spawns a child and waits in recv-match on a closure predicate runs to the end.
# docs/regions.md
#
# The trap: the scheduler holds the recv-match predicate across rounds and
# calls it again as each message arrives, and the async scheduler's pump
# closure must outlive the whole process run. A region that frees either
# closure early turns the next call into a call on a freed value.
#
# The shape needs all three: a spawned child that yields, a send, and a
# recv-match whose predicate is a closure. Each primitive is fetched with
# get, so every call site calls a closure read out of the module struct.

(def process ((import-file "lib/process.lisp")))

((get process :start) (fn []
                        (let [me ((get process :self))]
                          ((get process :send) ((get process :spawn) (fn []
                            (let [m ((get process :recv))]
                              ((get process :send) (get m 0) [:reply :tag :pong]))))
                          [me])
                          ((get process :recv-match) (fn [m]
                            (and (array? m) (= (get m 0) :reply)))))))

(println "region-spawn-recv-match: PASS")
