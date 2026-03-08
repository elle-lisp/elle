(defn f10 () 10)

(def result (fn/flow f10))
(display "type: ")
(display (type result))
(display "\n")

(display "keys: ")
(display (keys result))
(display "\n")

(def block0 (get (get result :blocks) 0))
(display "block0 keys: ")
(display (keys block0))
(display "\n")

(display "block0 :display type: ")
(display (type (get block0 :display)))
(display "\n")

(display "block0 :display value: ")
(display (get block0 :display))
(display "\n")
