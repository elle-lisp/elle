(elle/epoch 12)
# A spawned fiber outlives the parameterize scope it inherited from.
#
# fiber/new snapshots the creator's dynamic-parameter bindings at creation,
# precisely because the creator's parameterize blocks unwind long before the
# scheduler resumes the child. The snapshot must therefore COUNT what it
# holds (docs/impl/region/owner.md, "A child's inherited parameter baseline
# is a counted holder"): here the bound list's only other holder is
# spawn-reader's activation, which completes while the child sleeps. The
# child then reads the parameter after every structural holder is gone.
#
# Without the seeding retain, the child reads a freed region: debug builds
# panic at the resume boundary (the generation-stamped borrow check), and
# release builds read whatever reused the pages — the wrong-typed values
# and stalls the h2 corpus showed on the thread-pool backend.

(def p (make-parameter nil))

(defn spawn-reader []
  "Bind a fresh heap value for the child, spawn it, and return the fiber."
  (parameterize ((p (list "alive" 1 2 3)))
    (ev/spawn (fn []
                (ev/sleep 0.05)
                (first (p))))))

(def f (spawn-reader))
(let [[ok? v] (ev/join-protected f)]
  (assert ok? "child read its inherited parameter")
  (assert (= v "alive")
          (string "child saw the inherited value, got " (string v))))
(println "ok: inherited parameter survived its spawner")
