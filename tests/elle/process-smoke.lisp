(elle/epoch 9)
(def process ((import-file "lib/process.lisp")))
(def backend (*io-backend*))

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
      (let [val (process:recv)]
        (println (string "  ring: 0 → " val))
        (process:exit hog :kill)))))

(println "starting ring test")
(process:start run-ring :fuel 200 :backend backend)
(println "ring done")
