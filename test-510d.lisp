(defn f1 () 1)
(defn f2 () 2)
(defn f3 () 3)
(defn f4 () 4)
(defn f5 () 5)
(defn f6 () 6)
(defn f7 () 7)
(defn f8 () 8)
(defn f9 () 9)
(defn f10 () 10)

# Make 9 fn/cfg calls first
(fn/cfg f1)
(fn/cfg f2)
(fn/cfg f3)
(fn/cfg f4)
(fn/cfg f5)
(fn/cfg f6)
(fn/cfg f7)
(fn/cfg f8)
(fn/cfg f9)

# Now inspect what fn/flow returns for f10
(def result (fn/flow f10))
(display "type: ")
(display (type result))
(display "\n")

(display "keys: ")
(display (keys result))
(display "\n")

(display "name: ")
(display (get result :name))
(display "\n")

(display "arity: ")
(display (get result :arity))
(display "\n")

(display "blocks type: ")
(display (type (get result :blocks)))
(display "\n")

(display "blocks length: ")
(display (length (get result :blocks)))
(display "\n")

(def block0 (get (get result :blocks) 0))
(display "block0 type: ")
(display (type block0))
(display "\n")

(display "block0 keys: ")
(display (keys block0))
(display "\n")

(display "block0 :display type: ")
(display (type (get block0 :display)))
(display "\n")

(display "block0 :display value: ")
(display (get block0 :display))
(display "\n")

(display "block0 :instrs type: ")
(display (type (get block0 :instrs)))
(display "\n")

(display "block0 :annotated type: ")
(display (type (get block0 :annotated)))
(display "\n")

(display "block0 :term-kind: ")
(display (get block0 :term-kind))
(display "\n")
