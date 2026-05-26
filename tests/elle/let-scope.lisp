(elle/epoch 11)
## Tests: let binding value preservation
## Verifies that (let [x (f)] ... x) preserves x through the body.

## ── basic let + function call ────────────────────────────────

(defn identity [x]
  x)

(let [x (identity 42)]
  (assert (= x 42) "basic let identity"))

## ── let* with multiple bindings + function calls ─────────────

(defn add1 [x]
  (+ x 1))

(let* [a (identity 10)
       b (add1 a)
       c (add1 b)]
  (assert (= a 10) "let* first binding")
  (assert (= b 11) "let* second binding")
  (assert (= c 12) "let* third binding"))

## ── let with heap-allocated result ───────────────────────────

(defn make-vec []
  (let [buf @[1 2 3]]
    (freeze buf)))

(let [v (make-vec)]
  (assert (= (length v) 3) "let vec length")
  (assert (= (get v 0) 1) "let vec get"))

## ── let* with cleanup pattern (mirrors compress.lisp) ────────

(defn compute-result [n]
  (let [buf @[]]
    (each i in (range n)
      (push buf (* i 2)))
    (freeze buf)))

(defn process [n]
  (let* [raw (compute-result n)
         len (length raw)
         result (get raw 0)]
    (identity nil)
    (identity nil)
    (identity nil)
    result))

(assert (= (process 5) 0) "let* cleanup pattern")

## ── nested let with function call result ─────────────────────

(defn outer []
  (let [a (identity "hello")]
    (let [b (identity "world")]
      (let [c (string a " " b)]
        c))))

(assert (= (outer) "hello world") "nested let function calls")

## ── let inside loop body ─────────────────────────────────────

(defn collect-items [items]
  (let [result @[]]
    (each item in items
      (let [wrapped {"key" item}]
        (push result wrapped)))
    (freeze result)))

(let [out (collect-items ["x" "y" "z"])]
  (assert (= (length out) 3) "let in loop length")
  (assert (= (get (get out 0) "key") "x") "let in loop value"))

## ── let* where later bindings call functions ─────────────────

(defn make-bytes-like []
  (let [acc @[]]
    (each i in (range 5)
      (push acc i))
    (freeze acc)))

(defn decompress-like []
  (let* [data (make-bytes-like)
         len (length data)
         result (get data 2)]
    (identity nil)  ## cleanup-like
    (identity nil)
    result))

(assert (= (decompress-like) 2) "decompress-like pattern")

## ── deeply nested let with struct values ─────────────────────

(defn make-struct []
  {"a" 1 "b" 2 "c" 3})

(defn deep-let []
  (let [s (make-struct)]
    (let [a (get s "a")]
      (let [b (get s "b")]
        (let [c (get s "c")]
          (+ a b c))))))

(assert (= (deep-let) 6) "deeply nested let with struct")

(println "let-scope: all tests passed")
