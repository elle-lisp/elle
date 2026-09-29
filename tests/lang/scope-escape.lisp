(elle/epoch 12)
## Tests: values escaping scoped regions via push/get
## Verifies that values extracted from scope-local collections
## survive scope exit when pushed into outer collections.

## ── get from scope-local array, push to outer ────────────────

(defn test-get-push []
  (def acc @[])
  (def @idx 0)
  (while (%lt idx 1)
    (push acc (get (string/split "a b" " ") 1))
    (assign idx (%add idx 1)))
  (freeze acc))

(assert (= (test-get-push) ["b"]) "get+push through while scope")

## ── same pattern with each ──────────────────────────────────

(defn test-each-push []
  (def acc @[])
  (each item in ["a b"]
    (push acc (get (string/split item " ") 1)))
  (freeze acc))

(assert (= (test-each-push) ["b"]) "get+push through each scope")

## ── map with function that calls string/split + get ──────────

(assert (= (map (fn [l] (get (string/split l " ") 1)) ["a b"]) ["b"])
        "map with split+get")

## ── let-bound intermediate, push result to outer ─────────────

(defn test-let-push []
  (def acc @[])
  (each item in ["hello world" "foo bar"]
    (let [parts (string/split item " ")]
      (push acc (get parts 1))))
  (freeze acc))

(assert (= (test-let-push) ["world" "bar"]) "let-bound split, push get")

## ── multiple iterations ──────────────────────────────────────

(defn test-multi-iter []
  (def acc @[])
  (each item in ["a b" "c d" "e f"]
    (push acc (get (string/split item " ") 1)))
  (freeze acc))

(assert (= (test-multi-iter) ["b" "d" "f"]) "multi-iteration split+get")

## ── nested get from struct in array ──────────────────────────

(defn test-struct-get []
  (def validators (map (fn [p] {:check p :name "v"}) [integer? string?]))
  (def acc @[])
  (each i in (range (length validators))
    (let [v (get validators i)]
      (push acc (get v :name))))
  (freeze acc))

(assert (= (test-struct-get) ["v" "v"]) "struct get from array, push to outer")

## ── letrec + get pattern (contracts.lisp reproducer) ─────────

(defn test-letrec-get []
  (def vals (map (fn [p] {:check p}) [integer?]))
  (defn checker []
    (letrec [check (fn [i]
                     (when (< i 1)
                       (let [v (get vals i)]
                         (type v))
                       (check (+ i 1))))]
      (check 0)))
  (checker)
  (checker)
  (type (get vals 0)))

(assert (= (test-letrec-get) :struct) "letrec+get preserves struct type")

## ── dns.lisp parse-resolv-conf reproducer ────────────────────

(defn parse-resolv-conf [text]
  (let* [lines (map string/trim (string/split text "\n"))
         ns-lines (filter (fn [l] (string/starts-with? l "nameserver")) lines)
         addrs (map (fn [l]
                      (let [parts (string/split l " ")]
                        (when (>= (length parts) 2) (string/trim (get parts 1)))))
                    ns-lines)]
    (freeze (filter (fn [a] (and a (not (empty? a)))) addrs))))

(assert (= (parse-resolv-conf "nameserver 8.8.8.8\nnameserver 8.8.4.4\n")
           ["8.8.8.8" "8.8.4.4"]) "parse-resolv-conf: two servers")

(assert (= (parse-resolv-conf "# comment\nnameserver 1.1.1.1\nsearch example.com\n")
           ["1.1.1.1"]) "parse-resolv-conf: with comment")

(assert (empty? (parse-resolv-conf "")) "parse-resolv-conf: empty")

(println "scope-escape: all tests passed")
