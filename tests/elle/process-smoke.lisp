(elle/epoch 12)
# audited: 2026-09-23
# A process that spins without end cannot starve a ring of processes that pass a message around it.
# docs/processes.md

(def process ((import-file "lib/process.lisp")))

(defn run-ring []
  (let [me (process:self)]
    (defn make-node [next]
      (fn [] (process:send next (+ (process:recv) 1))))

    (let* [n3 (process:spawn (make-node me))
           n2 (process:spawn (make-node n3))
           n1 (process:spawn (make-node n2))
           hog (process:spawn (fn []
                                (letrec [spin (fn [n] (spin (+ n 1)))]
                                  (spin 0))))]
      (process:send n1 0)
      (assert (= (process:recv) 3)
              "the message went once round the ring of three")
      (process:exit hog :kill))))

(process:start run-ring :fuel 200)
(println "process-smoke: ok")
