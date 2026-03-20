(elle/epoch 1)
#!/usr/bin/env elle

# Destructuring — unpacking data at every level
#
# Demonstrates:
#   Silent nil semantics — missing elements become nil, not an error
#   Wildcard _           — discarding elements you don't need
#   List & rest          — collecting remaining list elements
#   Array & rest         — rest collects into an array
#   Nested patterns      — list-in-list, array-in-list, struct-in-struct
#   Mutable destructuring — var + set on destructured bindings
#   let / let*           — destructuring with sequential dependencies
#   Struct/@struct by-key  — extraction, missing keys, nested structs
#   Match dispatch       — struct tag patterns for polymorphic data



# ========================================
# 1. Strict destructuring semantics
# ========================================

# Destructuring errors on shape mismatch. Missing elements or keys → error.
# Extra elements are silently ignored.

# Fewer values than bindings: error (use match for optional elements)
(let (([ok? _] (protect ((fn () (def (sn-a sn-b sn-c) (list 10))))))) (assert (not ok?) "strict: missing elements => error"))
(def (sn-a) (list 10))
(assert (= sn-a 10) "strict: present element ok")

# More values than bindings: extras silently ignored (this is still fine)
(def (sn-p sn-q) (list 1 2 3 4 5))
(assert (= sn-q 2) "strict: extra elements ignored")

# Wrong type entirely: non-list signals error
(let (([ok? _] (protect ((fn () (def (sn-x sn-y) 42)))))) (assert (not ok?) "strict: non-list => error"))

# Array pattern on non-indexed value: error
(let (([ok? _] (protect ((fn () (def [sn-i sn-j] "hello")))))) (assert (not ok?) "strict: string in array pattern => error"))

(display "  strict:  (def (a b c) (list 10)) errors on missing element") (print "")
(display "  strict:  (def (x y) 42) errors on wrong type") (print "")


# ========================================
# 2. Wildcard _ pattern
# ========================================

# _ discards the matched value — no binding is created.
(def (_ wc-mid _) (list 10 20 30))
(assert (= wc-mid 20) "wildcard: skip first and third")

# Wildcard in array patterns
(def [_ wc-second _ wc-fourth] [100 200 300 400])
(assert (= wc-second 200) "wildcard: array skip")
(assert (= wc-fourth 400) "wildcard: array skip to fourth")

# Nested wildcard — skip outer, extract inner
(def ((_ wc-inner) _) (list (list :skip :want) :also-skip))
(assert (= wc-inner :want) "wildcard: nested extraction")

# Wildcard in struct pattern — acknowledge a key without binding it
(def {:x _ :y wc-y} {:x 10 :y 20})
(assert (= wc-y 20) "wildcard: struct skip value")

(display "  (def (_ mid _) (list 10 20 30)) → mid=") (print wc-mid)
(display "  (def ((_ inner) _) ...) → inner=") (print wc-inner)


# ========================================
# 3. List & rest
# ========================================

# & rest collects remaining list elements into a new list.
(def (lr-head & lr-tail) (list 1 2 3 4))
(assert (= lr-head 1) "list rest: head")
(assert (= (first lr-tail) 2) "list rest: tail first")
(assert (= (length lr-tail) 3) "list rest: tail length")

# All elements consumed: rest is empty list (not nil!)
(def (lr-a lr-b & lr-empty) (list 1 2))
(assert (empty? lr-empty) "list rest empty: rest is empty list")
(assert (not (nil? lr-empty)) "list rest empty: rest is NOT nil")

# Wildcard + rest: skip head, collect tail
(def (_ & lr-skip) (list :discard :keep1 :keep2))
(assert (= (first lr-skip) :keep1) "wildcard+rest: first of rest")

(display "  (h & t) from (1 2 3 4)    → h=") (display lr-head)
  (display " t=") (print lr-tail)
(display "  (a b & r) from (1 2)      → r=") (print lr-empty)


# ========================================
# 4. Tuple/array & rest
# ========================================

# Array rest collects remaining elements into an array.
(def [ar-first & ar-rest] [10 20 30])
(assert (= ar-first 10) "array rest: first")
(assert (= (get ar-rest 0) 20) "array rest: rest[0]")
(assert (array? ar-rest) "array rest: rest is array")

# Mutable array rest also collects into array
(def [mar-first & mar-rest] @[100 200 300])
(assert (= mar-first 100) "mutable array rest: first")

# Empty rest
(def [ar-only & ar-none] [42])
(assert (= (length ar-none) 0) "array rest empty: no remaining")

(display "  [a & r] from [10 20 30] → a=") (display ar-first)
  (display " r=") (display ar-rest)
  (display " array?=") (print (array? ar-rest))


# ========================================
# 5. Nested patterns
# ========================================

# List inside list — two levels deep
(def ((np-a np-b) np-c) (list (list 1 2) 3))
(assert (= np-a 1) "nested: list-in-list inner")
(assert (= np-c 3) "nested: list-in-list outer")

# Array inside list
(def ([np-x np-y] np-z) (list [10 20] 30))
(assert (= np-x 10) "nested: array-in-list inner")
(assert (= np-z 30) "nested: array-in-list outer")

# Struct inside struct
(def {:outer {:inner np-val}} {:outer {:inner 42}})
(assert (= np-val 42) "nested: struct-in-struct")

# Three levels: struct containing array containing a value we want
(def {:point [_ np-second]} {:point [:skip :target]})
(assert (= np-second :target) "nested: struct → array → element")

# Mixed: list of [name, {metadata}]
(def (np-name {:role np-role}) (list "Alice" {:role :admin :id 7}))
(assert (= np-name "Alice") "nested: mixed list+struct name")
(assert (= np-role :admin) "nested: mixed list+struct role")

(display "  nested list     → a=") (display np-a) (display " c=") (print np-c)
(display "  struct-in-struct → v=") (print np-val)
(display "  list+struct mix  → ") (display np-name) (display " ") (print np-role)


# ========================================
# 6. Mutable destructuring
# ========================================

# var + destructuring creates mutable bindings.
(var (mut-a mut-b) (list 1 2))
(assign mut-a 100)
(assert (= mut-a 100) "mutable: set after destructure")

# Works with arrays and structs too
(var [mut-x mut-y] [10 20])
(assign mut-x (+ mut-x mut-y))
(assert (= mut-x 30) "mutable: array set x = x + y")

(var {:count mut-count} {:count 0})
(assign mut-count (+ mut-count 1))
(assign mut-count (+ mut-count 1))
(assert (= mut-count 2) "mutable: struct incremented twice")

(display "  var list then set → a=") (print mut-a)
(display "  var struct, 2 increments → c=") (print mut-count)


# ========================================
# 7. let and let* destructuring
# ========================================

# Destructuring in let — parallel bindings
(def let-sum (let ([(la lb) (list 10 20)]) (+ la lb)))
(assert (= let-sum 30) "let: destructure sum")

# let* — sequential: second binding uses the first
(def star-seq (let* ([(sa sb) (list 1 2)]
                     [sc (+ sa sb)])
                sc))
(assert (= star-seq 3) "let*: sequential destructure")

# let* — chained destructuring, each level depends on previous
(def star-chain
  (let* ([(ca cb) (list 3 4)]
         [[cx cy] [ca cb]]
         [total (+ cx cy)])
    total))
(assert (= star-chain 7) "let*: chained destructure")

(display "  (let (((a b) (list 10 20))) (+ a b)) → ") (print let-sum)
(display "  (let* (((a b) ...) (c (+ a b))) c)   → ") (print star-seq)
(display "  chained let* across 3 bindings        → ") (print star-chain)


# ========================================
# 8. Struct/@struct by-key extraction
# ========================================

# Struct: extract named fields
(def {:name sk-name :age sk-age} {:name "Bob" :age 25})
(assert (= sk-name "Bob") "struct key: name")

# Missing key signals error
(let (([ok? _] (protect ((fn () (def {:missing sk-missing} {:other 42})))))) (assert (not ok?) "struct key: missing => error"))

# Non-struct signals error
(let (([ok? _] (protect ((fn () (def {:x sk-from-int} 42)))))) (assert (not ok?) "struct key: non-struct => error"))

# Mutable @struct works with the same pattern
(def {:a sk-from-tbl} @{:a 99 :b 100})
(assert (= sk-from-tbl 99) "@struct key: extract from mutable @struct")

# Nested struct extraction — three levels deep
(def {:config {:db {:host sk-host :port sk-port}}}
  {:config {:db {:host "localhost" :port 5432}}})
(assert (= sk-host "localhost") "nested struct: host")

# Struct destructuring in function parameters
(defn point-magnitude [{:x x :y y}]
  "Compute x + y from a point struct."
  (+ x y))
(assert (= (point-magnitude {:x 3 :y 4}) 7) "fn struct param")

# @struct in let
(def let-tbl (let ([{:a la :b lb} {:a 10 :b 20}]) (+ la lb)))
(assert (= let-tbl 30) "let: @struct destructure sum")

(display "  {:name n :age a} → ") (display sk-name) (display ", ") (print sk-age)
(display "  nested 3-level struct → host=") (print sk-host)
(display "  missing key → error (strict semantics)") (print "")


# ========================================
# 9. Match dispatch on struct tags
# ========================================

# Pattern matching on literal key values: tagged structs as sum types.
# {:type :circle :radius r} only matches when :type is literally :circle.

(defn area [shape]
  "Compute area from a tagged shape struct."
  (match shape
    ({:type :circle :radius r} (* r r))
    ({:type :square :side s}   (* s s))
    (_                         0)))

(assert (= (area {:type :circle :radius 5}) 25) "match dispatch: circle")
(assert (= (area {:type :square :side 7}) 49) "match dispatch: square")
(assert (= (area {:type :triangle}) 0) "match dispatch: fallback")
(assert (= (area 42) 0) "match dispatch: non-struct")

# Nested struct match — extract from inner structs
(defn db-host [config]
  "Extract host from a config struct, defaulting to unknown."
  (match config
    ({:db {:host h}} h)
    (_               "unknown")))

(assert (= (db-host {:db {:host "pg.local"}}) "pg.local") "match: nested struct")
(assert (= (db-host {:nodb true}) "unknown") "match: missing :db")

(display "  area(circle r=5) = ") (print (area {:type :circle :radius 5}))
(display "  area(square s=7) = ") (print (area {:type :square :side 7}))
(display "  db-host(pg)      = ") (print (db-host {:db {:host "pg"}}))


(print "")
(print "all destructuring passed.")
