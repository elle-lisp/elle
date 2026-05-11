(elle/epoch 10)
## Narrowing: while loop + string/split (no push)

## A: while with a non-allocating body
(defn test-a []
  (def @idx 0)
  (while (%lt idx 1)
    (+ 1 2)
    (assign idx (%add idx 1)))
  (println "A: arith in while ok"))
(test-a)

## B: while with string/split (allocates array)
(defn test-b []
  (def @idx 0)
  (while (%lt idx 1)
    (string/split "a b" " ")
    (assign idx (%add idx 1)))
  (println "B: split in while ok"))
(test-b)

## C: while with string/split and get
(defn test-c []
  (def @idx 0)
  (while (%lt idx 1)
    (get (string/split "a b" " ") 1)
    (assign idx (%add idx 1)))
  (println "C: split+get in while ok"))
(test-c)

## D: while with a heap-allocating call
(defn test-d []
  (def @idx 0)
  (while (%lt idx 1)
    (string "hello " "world")
    (assign idx (%add idx 1)))
  (println "D: string concat in while ok"))
(test-d)

## E: while with @array creation
(defn test-e []
  (def @idx 0)
  (while (%lt idx 1)
    @[1 2 3]
    (assign idx (%add idx 1)))
  (println "E: @array in while ok"))
(test-e)

(println "all done")
