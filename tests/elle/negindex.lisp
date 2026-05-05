(elle/epoch 10)
## Negative Indexing + Sequence Accessor Widening Tests

# ── get with negative indices ────────────────────────────────────────

# Arrays
(assert (= (get [10 20 30] -1) 30) "get array -1")
(assert (= (get [10 20 30] -2) 20) "get array -2")
(assert (= (get [10 20 30] -3) 10) "get array -len")
(assert (= (get [10 20 30] -4) nil) "get array -(len+1) oob")
(assert (= (get @[10 20 30] -1) 30) "get @array -1")
(assert (= (get @[10 20 30] -3) 10) "get @array -len")
(assert (= (get @[10 20 30] -4) nil) "get @array -(len+1) oob")

# Strings
(assert (= (get "hello" -1) "o") "get string -1")
(assert (= (get "hello" -5) "h") "get string -len")
(assert (= (get "hello" -6) nil) "get string -(len+1) oob")
(assert (= (get @"hello" -1) "o") "get @string -1")
(assert (= (get @"hello" -5) "h") "get @string -len")
(assert (= (get @"hello" -6) nil) "get @string -(len+1) oob")

# Bytes
(let [b (bytes 97 98 99)]
  (assert (= (get b -1) 99) "get bytes -1")
  (assert (= (get b -3) 97) "get bytes -len"))
(let [b (@bytes 97 98 99)]
  (assert (= (get b -1) 99) "get @bytes -1")
  (assert (= (get b -3) 97) "get @bytes -len"))

# Lists
(assert (= (get (list 10 20 30) -1) 30) "get list -1")
(assert (= (get (list 10 20 30) -3) 10) "get list -len")
(assert (= (get (list 10 20 30) -4) nil) "get list -(len+1) oob")

# Empty collections
(assert (= (get [] -1) nil) "get empty array -1")
(assert (= (get "" -1) nil) "get empty string -1")
(assert (= (get (list) -1) nil) "get empty list -1")

# Single element
(assert (= (get [42] -1) 42) "get single-element array -1")
(assert (= (get "x" -1) "x") "get single-char string -1")

# ── put with negative indices ────────────────────────────────────────

(assert (= (put [1 2 3] -1 99) [1 2 99]) "put array -1")
(assert (= (put [1 2 3] -3 99) [99 2 3]) "put array -len")
(assert (= (get (put @[1 2 3] -1 99) -1) 99) "put @array -1")
(assert (= (put "hello" -1 "a") "hella") "put string -1")
(assert (= (put "hello" -5 "H") "Hello") "put string -len")
(assert (= (freeze (begin
                     (def @s @"hello")
                     (put s -1 "X")
                     s)) "hellX") "put @string -1")

# ── callable form with negative index ────────────────────────────────

(assert (= ([10 20 30] -1) 30) "callable array -1")
(assert (= (@[10 20 30] -1) 30) "callable @array -1")
(assert (= ("food" -1) "d") "callable string -1")
(assert (= ((bytes 97 98 99) -1) 99) "callable bytes -1")
(assert (= ((@bytes 10 20 30) -1) 30) "callable @bytes -1")

# ── slice with negative indices ──────────────────────────────────────

(assert (= (slice [1 2 3 4 5] -3 5) [3 4 5]) "slice array neg start")
(assert (= (slice [1 2 3 4 5] 0 -2) [1 2 3]) "slice array neg end")
(assert (= (slice [1 2 3 4 5] -3 -1) [3 4]) "slice array both neg")
(assert (= (slice "hello" -3 -1) "ll") "slice string both neg")
(assert (= (slice (bytes 1 2 3 4 5) -2 5) (bytes 4 5)) "slice bytes neg start")
(assert (= (slice (list 1 2 3 4 5) -2 5) (list 4 5)) "slice list neg start")

# ── insert with negative index ───────────────────────────────────────

(assert (= (begin
             (def @a @[1 2 3])
             (insert a -1 99)
             a) @[1 2 99 3]) "insert @array -1")

# ── remove with negative index ───────────────────────────────────────

(assert (= (begin
             (def @a @[1 2 3])
             (remove a -1)
             a) @[1 2]) "remove @array -1")

# ── first on all sequence types ──────────────────────────────────────

(assert (= (first (list 1 2 3)) 1) "first list")
(assert (= (first [1 2 3]) 1) "first array")
(assert (= (first @[1 2 3]) 1) "first @array")
(assert (= (first "abc") "a") "first string")
(assert (= (first @"abc") "a") "first @string")
(assert (= (first (bytes 97 98 99)) 97) "first bytes")
(assert (= (first (@bytes 97 98 99)) 97) "first @bytes")

# first on empty → error
(let [[ok? _] (protect ((fn [] (first (list)))))]
  (assert (not ok?) "first empty list errors"))
(let [[ok? _] (protect ((fn [] (first []))))]
  (assert (not ok?) "first empty array errors"))
(let [[ok? _] (protect ((fn [] (first @[]))))]
  (assert (not ok?) "first empty @array errors"))
(let [[ok? _] (protect ((fn [] (first ""))))]
  (assert (not ok?) "first empty string errors"))
(let [[ok? _] (protect ((fn [] (first @""))))]
  (assert (not ok?) "first empty @string errors"))
(let [[ok? _] (protect ((fn [] (first (bytes)))))]
  (assert (not ok?) "first empty bytes errors"))
(let [[ok? _] (protect ((fn [] (first (@bytes)))))]
  (assert (not ok?) "first empty @bytes errors"))

# ── second on all sequence types ─────────────────────────────────────

(assert (= (second (list 1 2 3)) 2) "second list")
(assert (= (second [1 2 3]) 2) "second array")
(assert (= (second @[1 2 3]) 2) "second @array")
(assert (= (second "abc") "b") "second string")
(assert (= (second @"abc") "b") "second @string")
(assert (= (second (bytes 97 98 99)) 98) "second bytes")
(assert (= (second (@bytes 97 98 99)) 98) "second @bytes")

# second on empty/single → error
(let [[ok? _] (protect ((fn [] (second (list)))))]
  (assert (not ok?) "second empty list errors"))
(let [[ok? _] (protect ((fn [] (second (list 1)))))]
  (assert (not ok?) "second single list errors"))
(let [[ok? _] (protect ((fn [] (second []))))]
  (assert (not ok?) "second empty array errors"))
(let [[ok? _] (protect ((fn [] (second [1]))))]
  (assert (not ok?) "second single array errors"))
(let [[ok? _] (protect ((fn [] (second ""))))]
  (assert (not ok?) "second empty string errors"))
(let [[ok? _] (protect ((fn [] (second "a"))))]
  (assert (not ok?) "second single string errors"))
(let [[ok? _] (protect ((fn [] (second (bytes)))))]
  (assert (not ok?) "second empty bytes errors"))
(let [[ok? _] (protect ((fn [] (second (bytes 1)))))]
  (assert (not ok?) "second single bytes errors"))

# ── last on all sequence types ───────────────────────────────────────

(assert (= (last (list 1 2 3)) 3) "last list")
(assert (= (last [1 2 3]) 3) "last array")
(assert (= (last @[1 2 3]) 3) "last @array")
(assert (= (last "abc") "c") "last string")
(assert (= (last @"abc") "c") "last @string")
(assert (= (last (bytes 97 98 99)) 99) "last bytes")
(assert (= (last (@bytes 97 98 99)) 99) "last @bytes")

# last on empty → error
(let [[ok? _] (protect ((fn [] (last (list)))))]
  (assert (not ok?) "last empty list errors"))
(let [[ok? _] (protect ((fn [] (last []))))]
  (assert (not ok?) "last empty array errors"))
(let [[ok? _] (protect ((fn [] (last @[]))))]
  (assert (not ok?) "last empty @array errors"))
(let [[ok? _] (protect ((fn [] (last ""))))]
  (assert (not ok?) "last empty string errors"))
(let [[ok? _] (protect ((fn [] (last @""))))]
  (assert (not ok?) "last empty @string errors"))
(let [[ok? _] (protect ((fn [] (last (bytes)))))]
  (assert (not ok?) "last empty bytes errors"))
(let [[ok? _] (protect ((fn [] (last (@bytes)))))]
  (assert (not ok?) "last empty @bytes errors"))

# ── rest on all sequence types ───────────────────────────────────────

(assert (= (rest (list 1 2 3)) (list 2 3)) "rest list")
(assert (= (rest [1 2 3]) [2 3]) "rest array")
(assert (= (rest @[1 2 3]) @[2 3]) "rest @array")
(assert (= (rest "abc") "bc") "rest string")
(assert (= (freeze (rest @"abc")) "bc") "rest @string")
(assert (= (rest (bytes 97 98 99)) (bytes 98 99)) "rest bytes")
(assert (= (rest (@bytes 97 98 99)) (@bytes 98 99)) "rest @bytes")

# rest on empty → type-preserving empty
(assert (= (rest (list)) (list)) "rest empty list")
(assert (= (rest []) []) "rest empty array")
(assert (= (rest "") "") "rest empty string")
(assert (= (rest (bytes)) (bytes)) "rest empty bytes")

# ── butlast on all sequence types ────────────────────────────────────

(assert (= (butlast (list 1 2 3)) (list 1 2)) "butlast list")
(assert (= (butlast [1 2 3]) [1 2]) "butlast array")
(assert (= (butlast @[1 2 3]) @[1 2]) "butlast @array")
(assert (= (butlast "abc") "ab") "butlast string")
(assert (= (freeze (butlast @"abc")) "ab") "butlast @string")
(assert (= (butlast (bytes 97 98 99)) (bytes 97 98)) "butlast bytes")
(assert (= (butlast (@bytes 97 98 99)) (@bytes 97 98)) "butlast @bytes")

# butlast on empty → type-preserving empty
(assert (= (butlast (list)) (list)) "butlast empty list")
(assert (= (butlast []) []) "butlast empty array")
(assert (= (butlast "") "") "butlast empty string")
(assert (= (butlast (bytes)) (bytes)) "butlast empty bytes")

(println "all negative indexing + accessor widening tests passed")
