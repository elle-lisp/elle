(elle/epoch 10)
## Debug: trace scope-escape crash step by step

## Step 1: does string/split work?
(def parts (string/split "a b" " "))
(println "1. split:" parts "type:" (type parts))

## Step 2: does get work on the result?
(def b-val (get parts 1))
(println "2. get:" b-val "type:" (type b-val))

## Step 3: does push work?
(def acc @[])
(push acc b-val)
(println "3. push:" acc)

## Step 4: inline (no binding for split result)
(def acc2 @[])
(push acc2 (get (string/split "a b" " ") 1))
(println "4. inline push:" acc2)

## Step 5: inside a function (no loop)
(defn no-loop []
  (def a @[])
  (push a (get (string/split "a b" " ") 1))
  (println "5. fn no-loop:" a)
  (freeze a))
(no-loop)

## Step 6: function with while, but no push of split result
(defn loop-no-push []
  (def @idx 0)
  (while (%lt idx 1)
    (string/split "a b" " ")
    (assign idx (%add idx 1)))
  (println "6. loop-no-push: ok"))
(loop-no-push)

## Step 7: function with while, push literal
(defn loop-push-literal []
  (def a @[])
  (def @idx 0)
  (while (%lt idx 1)
    (push a "literal")
    (assign idx (%add idx 1)))
  (println "7. loop-push-literal:" a)
  (freeze a))
(loop-push-literal)

## Step 8: function with while, push variable from outside
(defn loop-push-outer []
  (def a @[])
  (def val (get (string/split "a b" " ") 1))
  (def @idx 0)
  (while (%lt idx 1)
    (push a val)
    (assign idx (%add idx 1)))
  (println "8. loop-push-outer:" a)
  (freeze a))
(loop-push-outer)

## Step 9: THE CRASH — push of inline split+get inside while
(defn loop-push-inline []
  (def a @[])
  (def @idx 0)
  (while (%lt idx 1)
    (push a (get (string/split "a b" " ") 1))
    (assign idx (%add idx 1)))
  (println "9. loop-push-inline:" a)
  (freeze a))
(loop-push-inline)

(println "all done")
