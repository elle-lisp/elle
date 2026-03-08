## Set types test suite

(import "examples/assertions.lisp")

## ── Literal syntax ──────────────────────────────────────────────────

(assert-eq || ||)                          # empty set
(assert-eq @|| @||)                        # empty mutable set
(assert-eq |1 2 3| |1 2 3|)               # immutable set
(assert-eq @|1 2 3| @|1 2 3|)             # mutable set
(assert-eq |3 1 2| |1 2 3|)               # order doesn't matter
(assert-eq |1 1 2| |1 2|)                 # deduplication

## ── Constructors ────────────────────────────────────────────────────

(assert-eq (set 1 2 3) |1 2 3|)
(assert-eq (mutable-set 1 2 3) @|1 2 3|)
(assert-eq (set) ||)
(assert-eq (mutable-set) @||)
(assert-eq (set 1 1 2 2 3) |1 2 3|)       # dedup in constructor

## ── Predicates ──────────────────────────────────────────────────────

(assert-true (set? |1 2 3|))
(assert-true (set? @|1 2 3|))             # set? covers both types
(assert-false (set? [1 2 3]))
(assert-false (set? "hello"))

## ── type-of distinguishes mutable from immutable ────────────────────

(assert-eq (type-of |1 2 3|) :set)
(assert-eq (type-of @|1 2 3|) :@set)

## ── Membership ──────────────────────────────────────────────────────

(assert-true (contains? |1 2 3| 2))
(assert-false (contains? |1 2 3| 4))
(assert-true (contains? @|1 2 3| 1))
(assert-false (contains? || 1))

## ── Element operations ──────────────────────────────────────────────

# add on immutable set returns new set
(assert-eq (add |1 2| 3) |1 2 3|)
(assert-eq (add |1 2| 2) |1 2|)           # no-op if already present

# add on mutable set mutates
(def ms @|1 2|)
(add ms 3)
(assert-true (contains? ms 3))

# del on immutable set returns new set
(assert-eq (del |1 2 3| 2) |1 3|)
(assert-eq (del |1 2 3| 4) |1 2 3|)       # no-op if not present

# del on mutable set mutates
(def ms2 @|1 2 3|)
(del ms2 2)
(assert-false (contains? ms2 2))

## ── Set algebra ─────────────────────────────────────────────────────

(assert-eq (union |1 2| |2 3|) |1 2 3|)
(assert-eq (intersection |1 2 3| |2 3 4|) |2 3|)
(assert-eq (difference |1 2 3| |2 3|) |1|)

# mutable set algebra
(assert-eq (union @|1 2| @|2 3|) @|1 2 3|)
(assert-eq (intersection @|1 2 3| @|2 3 4|) @|2 3|)
(assert-eq (difference @|1 2 3| @|2 3|) @|1|)

## ── Length and empty? ───────────────────────────────────────────────

(assert-eq (length |1 2 3|) 3)
(assert-eq (length ||) 0)
(assert-eq (length @|1 2|) 2)
(assert-true (empty? ||))
(assert-true (empty? @||))
(assert-false (empty? |1|))

## ── Freeze and thaw ────────────────────────────────────────────────

(assert-eq (freeze @|1 2 3|) |1 2 3|)
(assert-true (set? (freeze @|1 2 3|)))
(assert-eq (type-of (freeze @|1 2 3|)) :set)
(assert-eq (thaw |1 2 3|) @|1 2 3|)
(assert-true (set? (thaw |1 2 3|)))
(assert-eq (type-of (thaw |1 2 3|)) :@set)

## ── Conversion ──────────────────────────────────────────────────────

(assert-eq (length (set->list |3 1 2|)) 3)

## ── Freeze on insert ────────────────────────────────────────────────

# Mutable values are frozen when inserted
(assert-true (contains? (set @[1 2]) [1 2]))
(assert-true (contains? |1 2 3| 2))

## ── Match type guards ───────────────────────────────────────────────

(assert-eq (match |1 2 3|
             (|s| (length s)))
           3)

(assert-eq (match @|1 2|
             (@|s| (length s)))
           2)

(assert-eq (match |1 2 3|
             (@|s| :mutable)
             (|s| :immutable))
           :immutable)

## ── Each iteration ──────────────────────────────────────────────────

(var sum 0)
(each x |1 2 3|
  (assign sum (+ sum x)))
(assert-eq sum 6)

## ── Map ─────────────────────────────────────────────────────────────

(def doubled (map (fn (x) (* x 2)) |1 2 3|))
(assert-true (set? doubled))
(assert-true (contains? doubled 2))
(assert-true (contains? doubled 4))
(assert-true (contains? doubled 6))

## ── Display ─────────────────────────────────────────────────────────

(assert-eq (string/format "{}" ||) "||")
(assert-eq (string/format "{}" @||) "@||")
