## Set types test suite

(import "examples/assertions.lisp")

## ── Literal syntax ──────────────────────────────────────────────────

(assert-eq || || "test")                          # empty set
(assert-eq @|| @|| "test")                        # empty mutable set
(assert-eq |1 2 3| |1 2 3| "test")               # immutable set
(assert-eq @|1 2 3| @|1 2 3| "test")             # mutable set
(assert-eq |3 1 2| |1 2 3| "test")               # order doesn't matter
(assert-eq |1 1 2| |1 2| "test")                 # deduplication

## ── Constructors ────────────────────────────────────────────────────

(assert-eq (set 1 2 3) |1 2 3| "test")
(assert-eq (@set 1 2 3) @|1 2 3| "test")
(assert-eq (set) || "test")
(assert-eq (@set) @|| "test")
(assert-eq (set 1 1 2 2 3) |1 2 3| "test")       # dedup in constructor

## ── Predicates ──────────────────────────────────────────────────────

(assert-true (set? |1 2 3|) "test")
(assert-true (set? @|1 2 3|) "test")             # set? covers both types
(assert-false (set? [1 2 3]) "test")
(assert-false (set? "hello") "test")

## ── type-of distinguishes mutable from immutable ────────────────────

(assert-eq (type-of |1 2 3|) :set "test")
(assert-eq (type-of @|1 2 3|) :@set "test")

## ── Membership ──────────────────────────────────────────────────────

(assert-true (contains? |1 2 3| 2) "test")
(assert-false (contains? |1 2 3| 4) "test")
(assert-true (contains? @|1 2 3| 1) "test")
(assert-false (contains? || 1) "test")

## ── Element operations ──────────────────────────────────────────────

# add on immutable set returns new set
(assert-eq (add |1 2| 3) |1 2 3| "test")
(assert-eq (add |1 2| 2) |1 2| "test")           # no-op if already present

# add on mutable set mutates
(def ms @|1 2|)
(add ms 3)
(assert-true (contains? ms 3) "test")

# del on immutable set returns new set
(assert-eq (del |1 2 3| 2) |1 3| "test")
(assert-eq (del |1 2 3| 4) |1 2 3| "test")       # no-op if not present

# del on mutable set mutates
(def ms2 @|1 2 3|)
(del ms2 2)
(assert-false (contains? ms2 2) "test")

## ── Set algebra ─────────────────────────────────────────────────────

(assert-eq (union |1 2| |2 3|) |1 2 3| "test")
(assert-eq (intersection |1 2 3| |2 3 4|) |2 3| "test")
(assert-eq (difference |1 2 3| |2 3|) |1| "test")

# mutable set algebra
(assert-eq (union @|1 2| @|2 3|) @|1 2 3| "test")
(assert-eq (intersection @|1 2 3| @|2 3 4|) @|2 3| "test")
(assert-eq (difference @|1 2 3| @|2 3|) @|1| "test")

## ── Length and empty? ───────────────────────────────────────────────

(assert-eq (length |1 2 3|) 3 "test")
(assert-eq (length ||) 0 "test")
(assert-eq (length @|1 2|) 2 "test")
(assert-true (empty? ||) "test")
(assert-true (empty? @||) "test")
(assert-false (empty? |1|) "test")

## ── Freeze and thaw ────────────────────────────────────────────────

(assert-eq (freeze @|1 2 3|) |1 2 3| "test")
(assert-true (set? (freeze @|1 2 3|)) "test")
(assert-eq (type-of (freeze @|1 2 3|)) :set "test")
(assert-eq (thaw |1 2 3|) @|1 2 3| "test")
(assert-true (set? (thaw |1 2 3|)) "test")
(assert-eq (type-of (thaw |1 2 3|)) :@set "test")

## ── Conversion ──────────────────────────────────────────────────────

(assert-eq (length (set->array |3 1 2|)) 3 "test")

## ── Freeze on insert ────────────────────────────────────────────────

# Mutable values are frozen when inserted
(assert-true (contains? (set @[1 2]) [1 2]) "test")
(assert-true (contains? |1 2 3| 2) "test")

## ── Match type guards ───────────────────────────────────────────────

(assert-eq (match |1 2 3|
             (|s| (length s))
             (_ :no-match))
           3
           "match immutable set")

(assert-eq (match @|1 2|
             (@|s| (length s))
             (_ :no-match))
           2
           "match mutable set")

(assert-eq (match |1 2 3|
             (@|s| :mutable)
             (|s| :immutable)
             (_ :no-match))
           :immutable
           "match distinguishes set types")

## ── Each iteration ──────────────────────────────────────────────────

(var sum 0)
(each x |1 2 3|
  (assign sum (+ sum x)))
(assert-eq sum 6 "test")

## ── Map ─────────────────────────────────────────────────────────────

(def doubled (map (fn (x) (* x 2)) |1 2 3|))
(assert-true (set? doubled) "test")
(assert-true (contains? doubled 2) "test")
(assert-true (contains? doubled 4) "test")
(assert-true (contains? doubled 6) "test")

## ── Display ─────────────────────────────────────────────────────────

(assert-eq (string/format "{}" ||) "||" "test")
(assert-eq (string/format "{}" @||) "@||" "test")
