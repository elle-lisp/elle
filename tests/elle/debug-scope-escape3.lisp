(elle/epoch 10)

## Is it string/split specifically, or any heap alloc in while?

(defn test-string-concat []
  (def @idx 0)
  (while (%lt idx 3)
    (string "hello" "world")
    (assign idx (%add idx 1)))
  (println "string concat ok"))
(test-string-concat)

(defn test-array-literal []
  (def @idx 0)
  (while (%lt idx 3)
    [1 2 3]
    (assign idx (%add idx 1)))
  (println "array literal ok"))
(test-array-literal)

(defn test-struct-literal []
  (def @idx 0)
  (while (%lt idx 3)
    {:a 1 :b 2}
    (assign idx (%add idx 1)))
  (println "struct literal ok"))
(test-struct-literal)

(defn test-bytes []
  (def @idx 0)
  (while (%lt idx 3)
    (bytes 1 2 3)
    (assign idx (%add idx 1)))
  (println "bytes ok"))
(test-bytes)

(defn test-split []
  (def @idx 0)
  (while (%lt idx 3)
    (string/split "a b" " ")
    (assign idx (%add idx 1)))
  (println "string/split ok"))
(test-split)

(println "all done")
