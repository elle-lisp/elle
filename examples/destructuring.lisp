#!/usr/bin/env elle

# Destructuring — unpacking data at every level
#
# Demonstrates:
#   Silent nil semantics — missing elements become nil, not an error
#   Wildcard _           — discarding elements you don't need
#   List & rest          — collecting remaining list elements
#   Tuple/array & rest   — rest collects into an array
#   Nested patterns      — list-in-list, tuple-in-list, struct-in-struct
#   Mutable destructuring — var + set on destructured bindings
#   let / let*           — destructuring with sequential dependencies
#   Struct/table by-key  — extraction, missing keys, nested structs
#   Match dispatch       — struct tag patterns for polymorphic data

(import-file "./examples/assertions.lisp")


# ========================================
# 1. Silent nil semantics
# ========================================

# Destructuring never errors on shape mismatch. Missing → nil.

# Fewer values than bindings: extras get nil
(def (sn-a sn-b sn-c) (list 10))
(assert-eq sn-a 10 "silent nil: present element")
(assert-eq sn-b nil "silent nil: missing => nil")

# More values than bindings: extras silently ignored
(def (sn-p sn-q) (list 1 2 3 4 5))
(assert-eq sn-q 2 "silent nil: extra elements ignored")

# Wrong type entirely: non-list gives nil for all bindings
(def (sn-x sn-y) 42)
(assert-eq sn-x nil "silent nil: non-list => nil")

# Same for tuple patterns on a non-indexed value
(def [sn-i sn-j] "hello")
(assert-eq sn-i nil "silent nil: string in tuple pattern => nil")

(display "  fewer vals:  (def (a b c) (list 10)) → b=") (print sn-b)
(display "  wrong type:  (def (x y) 42)          → x=") (print sn-x)


# ========================================
# 2. Wildcard _ pattern
# ========================================

# _ discards the matched value — no binding is created.
(def (_ wc-mid _) (list 10 20 30))
(assert-eq wc-mid 20 "wildcard: skip first and third")

# Wildcard in tuple patterns
(def [_ wc-second _ wc-fourth] [100 200 300 400])
(assert-eq wc-second 200 "wildcard: tuple skip")
(assert-eq wc-fourth 400 "wildcard: tuple skip to fourth")

# Nested wildcard — skip outer, extract inner
(def ((_ wc-inner) _) (list (list :skip :want) :also-skip))
(assert-eq wc-inner :want "wildcard: nested extraction")

# Wildcard in struct pattern — acknowledge a key without binding it
(def {:x _ :y wc-y} {:x 10 :y 20})
(assert-eq wc-y 20 "wildcard: struct skip value")

(display "  (def (_ mid _) (list 10 20 30)) → mid=") (print wc-mid)
(display "  (def ((_ inner) _) ...) → inner=") (print wc-inner)


# ========================================
# 3. List & rest
# ========================================

# & rest collects remaining list elements into a new list.
(def (lr-head & lr-tail) (list 1 2 3 4))
(assert-eq lr-head 1 "list rest: head")
(assert-eq (first lr-tail) 2 "list rest: tail first")
(assert-eq (length lr-tail) 3 "list rest: tail length")

# All elements consumed: rest is empty list (not nil!)
(def (lr-a lr-b & lr-empty) (list 1 2))
(assert-true (empty? lr-empty) "list rest empty: rest is empty list")
(assert-false (nil? lr-empty) "list rest empty: rest is NOT nil")

# Wildcard + rest: skip head, collect tail
(def (_ & lr-skip) (list :discard :keep1 :keep2))
(assert-eq (first lr-skip) :keep1 "wildcard+rest: first of rest")

(display "  (h & t) from (1 2 3 4)    → h=") (display lr-head)
  (display " t=") (print lr-tail)
(display "  (a b & r) from (1 2)      → r=") (print lr-empty)


# ========================================
# 4. Tuple/array & rest
# ========================================

# Tuple rest collects remaining elements into an *array* (not a tuple).
(def [tr-first & tr-rest] [10 20 30])
(assert-eq tr-first 10 "tuple rest: first")
(assert-eq (get tr-rest 0) 20 "tuple rest: rest[0]")
(assert-true (array? tr-rest) "tuple rest: rest is array, not tuple")

# Array rest also collects into array
(def [ar-first & ar-rest] @[100 200 300])
(assert-eq ar-first 100 "array rest: first")

# Empty rest
(def [tr-only & tr-none] [42])
(assert-eq (length tr-none) 0 "tuple rest empty: no remaining")

(display "  [a & r] from [10 20 30] → a=") (display tr-first)
  (display " r=") (display tr-rest)
  (display " array?=") (print (array? tr-rest))


# ========================================
# 5. Nested patterns
# ========================================

# List inside list — two levels deep
(def ((np-a np-b) np-c) (list (list 1 2) 3))
(assert-eq np-a 1 "nested: list-in-list inner")
(assert-eq np-c 3 "nested: list-in-list outer")

# Tuple inside list
(def ([np-x np-y] np-z) (list [10 20] 30))
(assert-eq np-x 10 "nested: tuple-in-list inner")
(assert-eq np-z 30 "nested: tuple-in-list outer")

# Struct inside struct
(def {:outer {:inner np-val}} {:outer {:inner 42}})
(assert-eq np-val 42 "nested: struct-in-struct")

# Three levels: struct containing tuple containing a value we want
(def {:point [_ np-second]} {:point [:skip :target]})
(assert-eq np-second :target "nested: struct → tuple → element")

# Mixed: list of [name, {metadata}]
(def (np-name {:role np-role}) (list "Alice" {:role :admin :id 7}))
(assert-eq np-name "Alice" "nested: mixed list+struct name")
(assert-eq np-role :admin "nested: mixed list+struct role")

(display "  nested list     → a=") (display np-a) (display " c=") (print np-c)
(display "  struct-in-struct → v=") (print np-val)
(display "  list+struct mix  → ") (display np-name) (display " ") (print np-role)


# ========================================
# 6. Mutable destructuring
# ========================================

# var + destructuring creates mutable bindings.
(var (mut-a mut-b) (list 1 2))
(set mut-a 100)
(assert-eq mut-a 100 "mutable: set after destructure")

# Works with tuples and structs too
(var [mut-x mut-y] [10 20])
(set mut-x (+ mut-x mut-y))
(assert-eq mut-x 30 "mutable: tuple set x = x + y")

(var {:count mut-count} {:count 0})
(set mut-count (+ mut-count 1))
(set mut-count (+ mut-count 1))
(assert-eq mut-count 2 "mutable: struct incremented twice")

(display "  var list then set → a=") (print mut-a)
(display "  var struct, 2 increments → c=") (print mut-count)


# ========================================
# 7. let and let* destructuring
# ========================================

# Destructuring in let — parallel bindings
(def let-sum (let ([(la lb) (list 10 20)]) (+ la lb)))
(assert-eq let-sum 30 "let: destructure sum")

# let* — sequential: second binding uses the first
(def star-seq (let* ([(sa sb) (list 1 2)]
                     [sc (+ sa sb)])
                sc))
(assert-eq star-seq 3 "let*: sequential destructure")

# let* — chained destructuring, each level depends on previous
(def star-chain
  (let* ([(ca cb) (list 3 4)]
         [[cx cy] [ca cb]]
         [total (+ cx cy)])
    total))
(assert-eq star-chain 7 "let*: chained destructure")

(display "  (let (((a b) (list 10 20))) (+ a b)) → ") (print let-sum)
(display "  (let* (((a b) ...) (c (+ a b))) c)   → ") (print star-seq)
(display "  chained let* across 3 bindings        → ") (print star-chain)


# ========================================
# 8. Struct/table by-key extraction
# ========================================

# Struct: extract named fields
(def {:name sk-name :age sk-age} {:name "Bob" :age 25})
(assert-eq sk-name "Bob" "struct key: name")

# Missing key gives nil
(def {:missing sk-missing} {:other 42})
(assert-eq sk-missing nil "struct key: missing => nil")

# Non-struct gives nil for all bindings
(def {:x sk-from-int} 42)
(assert-eq sk-from-int nil "struct key: non-struct => nil")

# Mutable table works with the same pattern
(def {:a sk-from-tbl} @{:a 99 :b 100})
(assert-eq sk-from-tbl 99 "table key: extract from mutable table")

# Nested struct extraction — three levels deep
(def {:config {:db {:host sk-host :port sk-port}}}
  {:config {:db {:host "localhost" :port 5432}}})
(assert-eq sk-host "localhost" "nested struct: host")

# Struct destructuring in function parameters
(defn point-magnitude [{:x x :y y}]
  "Compute x + y from a point struct."
  (+ x y))
(assert-eq (point-magnitude {:x 3 :y 4}) 7 "fn struct param")

# Table in let
(def let-tbl (let ([{:a la :b lb} {:a 10 :b 20}]) (+ la lb)))
(assert-eq let-tbl 30 "let: table destructure sum")

(display "  {:name n :age a} → ") (display sk-name) (display ", ") (print sk-age)
(display "  nested 3-level struct → host=") (print sk-host)
(display "  missing key → ") (print sk-missing)


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

(assert-eq (area {:type :circle :radius 5}) 25 "match dispatch: circle")
(assert-eq (area {:type :square :side 7})   49 "match dispatch: square")
(assert-eq (area {:type :triangle})          0 "match dispatch: fallback")
(assert-eq (area 42)                         0 "match dispatch: non-struct")

# Nested struct match — extract from inner structs
(defn db-host [config]
  "Extract host from a config struct, defaulting to unknown."
  (match config
    ({:db {:host h}} h)
    (_               "unknown")))

(assert-eq (db-host {:db {:host "pg.local"}}) "pg.local" "match: nested struct")
(assert-eq (db-host {:nodb true})             "unknown" "match: missing :db")

(display "  area(circle r=5) = ") (print (area {:type :circle :radius 5}))
(display "  area(square s=7) = ") (print (area {:type :square :side 7}))
(display "  db-host(pg)      = ") (print (db-host {:db {:host "pg"}}))


(print "")
(print "all destructuring passed.")
