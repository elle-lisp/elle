(import-file "tests/elle/assert.lisp")

## fn/flow integration tests
## Tests the control flow graph introspection

(assert-true (struct? (fn/flow (fn (x) x))) "fn/flow returns struct")

(let ((cfg (fn/flow (fn (x y) (+ x y)))))
  (assert-true (not (nil? (get cfg :arity))) "fn/flow has arity")
  (assert-true (not (nil? (get cfg :regs))) "fn/flow has regs")
  (assert-true (not (nil? (get cfg :locals))) "fn/flow has locals")
  (assert-true (not (nil? (get cfg :entry))) "fn/flow has entry")
  (assert-true (not (nil? (get cfg :blocks))) "fn/flow has blocks"))

(assert-true (string? (get (fn/flow (fn (x y) (+ x y))) :arity)) "fn/flow arity is string")

(assert-eq (get (fn/flow (fn (x y) (+ x y))) :arity) "2" "fn/flow arity value")

(assert-true (tuple? (get (fn/flow (fn (x) x)) :blocks)) "fn/flow blocks is tuple")

(assert-true (> (length (get (fn/flow (fn (x) x)) :blocks)) 0) "fn/flow blocks nonempty")

(let ((block (get (get (fn/flow (fn (x) x)) :blocks) 0)))
  (assert-true (not (nil? (get block :label))) "fn/flow block has label")
  (assert-true (not (nil? (get block :instrs))) "fn/flow block has instrs")
  (assert-true (not (nil? (get block :term))) "fn/flow block has term")
  (assert-true (not (nil? (get block :edges))) "fn/flow block has edges"))

(let ((instrs (get (get (get (fn/flow (fn (x) x)) :blocks) 0) :instrs)))
  (assert-true (tuple? instrs) "fn/flow instrs is tuple of strings"))

(let ((edges (get (get (get (fn/flow (fn (x) x)) :blocks) 0) :edges)))
  (assert-true (tuple? edges) "fn/flow edges is tuple"))

(let ((term (get (get (get (fn/flow (fn (x) x)) :blocks) 0) :term)))
  (assert-true (string? term) "fn/flow term is string"))

(assert-err (fn () (fn/flow 42)) "fn/flow non-closure errors")

(defn my-add (x y) (+ x y))
(assert-true (nil? (get (fn/flow my-add) :name)) "fn/flow named function")

(assert-true (nil? (get (fn/flow (fn (x) x)) :name)) "fn/flow anonymous function name is nil")

(defn my-add (x y) "Add two numbers." (+ x y))
(assert-eq (get (fn/flow my-add) :doc) "Add two numbers." "fn/flow doc with docstring")

(assert-true (nil? (get (fn/flow (fn (x) x)) :doc)) "fn/flow doc without docstring")

(let ((cfg (fn/flow (fn (x) (if x 1 2)))))
  (let ((blocks (get cfg :blocks)))
    (assert-true (> (length blocks) 1) "fn/flow branching has edges")))
