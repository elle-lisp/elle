(elle/epoch 13)
# audited: 2026-09-29
# Guard for the macro-expansion arena: a transformer's values live until its expansion ends.
# docs/impl/region/macroscope.md
#
# While a macro expands, every value its transformer allocates joins one arena,
# and the expansion's close frees the arena once the result is copied to syntax.
# These transformers build nested templates, run closures, catch errors, and
# expand into further macro calls. Each expansion then runs, after region-id
# churn, and so does an `eval` that expands the same macros at run time. A value
# freed before the copy faults under --trace=guardfree.

(defn churn []
  (var i 0)
  (while (< i 300)
    (pair (string "c" i) i)
    (assign i (+ i 1))))

(defmacro nest (a b)
  `(list (array ,a ,b) (struct :a ,a :b (array ,b ,a)) ,(string "lit" a)))

(defmacro via-closure (x)
  (let [wrap (fn [y] `(+ ,y 1))]
    (wrap (wrap x))))

(defmacro guarded (x)
  (try
    `(quote ,(first x))
    (catch e `:not-a-list)))

(defmacro chain (& xs)
  (if (empty? xs)
    `()
    `(pair ,(first xs) (chain ,;(rest xs)))))

(defmacro built-by-loop (n)
  (let [out @[]]
    (var k 0)
    (while (< k n)
      (push out k)
      (assign k (+ k 1)))
    `(array ,;(apply list out))))

(def nested (nest 1 2))
(churn)
(assert (= (get (first nested) 1) 2) "a nested template expanded whole")
(assert (= (get (get (first (rest nested)) :b) 1) 1) "and its struct")
(assert (= (first (rest (rest nested))) "lit1") "and its computed literal")

(assert (= (via-closure 5) 7) "a transformer's closures run before the close")
(assert (= (guarded (a b)) 'a) "a list argument expands")
(assert (= (guarded 7) :not-a-list) "a caught error expands too")
(assert (= (first (rest (chain 1 2 3))) 2) "an expansion that expands again")
(assert (= (get (built-by-loop 5) 4) 4) "a transformer's own container")

# Macros defined and expanded at run time, many times over. `eval` compiles
# against the prelude, so the program carries its own definitions, and they
# build their expansions with `list`: `eval` of quoted data does not expand a
# quasiquote.
(def program
  '(begin
     (defmacro wrap-twice (x)
       (let [wrap (fn [y] (list '+ y 1))]
         (wrap (wrap x))))
     (defmacro counted (n)
       (let [out @[]]
         (var k 0)
         (while (< k n)
           (push out k)
           (assign k (+ k 1)))
         (pair 'array (apply list out))))
     (+ (wrap-twice 1) (get (counted 3) 2) (length (when true (list 1 2 3 4))))))
(var total 0)
(var round 0)
(while (< round 40)
  (assign total (+ total (eval program)))
  (assign round (+ round 1)))
(assert (= total (* 40 (+ 3 2 4))) "every run-time expansion ran whole")

(println "region-macro-arena-uaf: ok")
