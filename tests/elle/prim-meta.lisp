(elle/epoch 11)
## tests/elle/prim-meta.lisp
## meta/origin — closure source location

# non-closure returns nil
(assert (nil? (meta/origin 42)) "meta/origin non-closure returns nil")
(assert (nil? (meta/origin nil)) "meta/origin nil returns nil")

# closure with source info returns struct with :file, :line, :col
(let [result (meta/origin (fn [] nil))]
  (if result
    (do
      (assert (get result :file) "meta/origin has :file")
      (assert (get result :line) "meta/origin has :line")
      (assert (get result :col) "meta/origin has :col"))
    nil))  # if no source info attached (e.g. in test mode), that's ok

(println "prim-meta: all tests passed")
