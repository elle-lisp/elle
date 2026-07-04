(elle/epoch 12)
## Match Reachability and No-Match Semantics
##
## Unreachable arms — arms the decision tree proves no value can reach —
## are compile-time errors. A match with no catch-all compiles; an
## unmatched value raises a runtime :match-error carrying the value.

# ============================================================================
# Unreachable arms are compile-time errors
# ============================================================================

# arm after a guardless wildcard is unreachable
(let [[ok? _] (protect ((fn ()
                          (eval '(match 1
                                   _ :a
                                   1 :b)))))]
  (assert (not ok?) "arm after wildcard is a compile error"))

# arm after a guardless variable catch-all is unreachable
(let [[ok? _] (protect ((fn ()
                          (eval '(match 1
                                   x :a
                                   1 :b)))))]
  (assert (not ok?) "arm after variable catch-all is a compile error"))

# duplicate literal arm is unreachable
(let [[ok? _] (protect ((fn ()
                          (eval '(match 1
                                   1 :a
                                   1 :b
                                   _ :c)))))]
  (assert (not ok?) "duplicate literal arm is a compile error"))

# arm fully covered by an earlier or-pattern is unreachable
(let [[ok? _] (protect ((fn ()
                          (eval '(match 2
                                   (or 1 2) :a
                                   2 :b
                                   _ :c)))))]
  (assert (not ok?) "arm covered by earlier or-pattern is a compile error"))

# ============================================================================
# Guards never make later arms unreachable
# ============================================================================

# guarded catch-all does not kill later arms
(assert (= (match 1
             x when
             false :never
             1 :b) :b) "guarded catch-all keeps later arms reachable")

# guarded duplicate literal stays reachable
(assert (= (match 1
             1 when
             false :a
             1 :b) :b) "guarded duplicate literal stays reachable")

# ============================================================================
# Or-pattern redundancy: every alternative must add coverage beyond
# earlier arms and earlier alternatives, at any nesting depth
# ============================================================================

# alternative covered by an earlier arm is a compile error
(let [[ok? _] (protect ((fn ()
                          (eval '(match 2
                                   1 :a
                                   (or 1 2) :b)))))]
  (assert (not ok?) "or-alternative covered by earlier arm is a compile error"))

# duplicate alternative within one or-pattern is a compile error
(let [[ok? _] (protect ((fn ()
                          (eval '(match 1
                                   (or 1 1) :a
                                   _ :b)))))]
  (assert (not ok?) "duplicate or-alternative is a compile error"))

# alternative covered by an earlier wildcard alternative is a compile error
(let [[ok? _] (protect ((fn ()
                          (eval '(match 1
                                   (or _ 1) :a)))))]
  (assert (not ok?) "alternative after wildcard alternative is a compile error"))

# nested: dead alternative inside a compound pattern is a compile error
(let [[ok? _] (protect ((fn ()
                          (eval '(match (pair 1 2)
                                   (1 . (or _ 2)) :a
                                   _ :b)))))]
  (assert (not ok?) "dead nested or-alternative is a compile error"))

# both alternatives cover all pairs: the second is dead
(let [[ok? _] (protect ((fn ()
                          (eval '(match (pair 1 2)
                                   (or (x . _) (_ . x)) x)))))]
  (assert (not ok?) "pair-shadowed or-alternative is a compile error"))

# all alternatives contribute: compiles and matches
(assert (= (match 2
             3 :a
             (or 1 2) :b) :b) "or-pattern with live alternatives compiles")

# sibling or-patterns are independent: all four alternatives contribute
(assert (= (match (pair 2 :b)
             ((or 1 2) . (or :a :b)) :hit
             _ :miss) :hit) "sibling or-patterns stay independent")

# a guarded earlier arm does not kill an alternative (guard may fail)
(assert (= (match 1
             x when
             false :never
             (or 1 2) :b) :b) "guarded arm keeps or-alternatives reachable")

# on a guarded arm, earlier alternatives never kill later ones: a
# failed guard retries the remaining alternatives with fresh bindings
(assert (= (match (pair :a 5)
             (or (x . _) (_ . x)) when
             (= x 5) x
             _ :none) 5) "failed guard retries later or-alternatives")

# every alternative failing the guard falls through to :match-error
(let [[ok? err] (protect (match (pair :a :b)
                           (or (x . _) (_ . x)) when
                           (= x 99) x))]
  (assert (not ok?) "all alternatives failing the guard raises")
  (assert (= (get err :error) :match-error) "guard-exhausted error kind"))

# nested or inside an or-alternative: the ancestor's earlier
# alternatives count as coverage
(let [[ok? _] (protect ((fn ()
                          (eval '(match 1
                                   (or 1 (or 1 2)) :a)))))]
  (assert (not ok?)
          "alternative dead via ancestor or-alternative is a compile error"))

# ============================================================================
# Signal accounting: a match that can fail is typed as may-error
# ============================================================================

# a match without an irrefutable guardless arm violates (silent!)
(let [[ok? _] (protect ((fn ()
                          (eval '(defn silent-match-hole (x)
                                  (silent!)
                                  (match x
                                    1 :one))))))]
  (assert (not ok?) "non-exhaustive match violates silent!"))

# with a catch-all arm the match is silent
(defn silent-match-ok (x)
  (silent!)
  (match x
    1 :one
    _ :other))
(assert (= (silent-match-ok 2) :other) "catch-all match satisfies silent!")

# ============================================================================
# No catch-all: unmatched values raise :match-error at runtime
# ============================================================================

# single-arm match works when it matches
(assert (= (match 1
             1 :one) :one) "single-arm match matches")

# single-arm match raises when it doesn't
(let [[ok? err] (protect (match 2
                           1 :one))]
  (assert (not ok?) "single-arm no-match raises")
  (assert (= (get err :error) :match-error) "single-arm error kind")
  (assert (= (get err :value) 2) "single-arm error carries the value"))

# bool arms on a non-bool scrutinee raise (was the silent-nil hole)
(let [[ok? err] (protect (match 5
                           true 1
                           false 2))]
  (assert (not ok?) "bool arms on non-bool scrutinee raise")
  (assert (= (get err :error) :match-error) "bool-hole error kind")
  (assert (= (get err :value) 5) "bool-hole error carries the value"))

# structural no-match carries the whole scrutinee
(let [[ok? err] (protect (match (list 1 2)
                           (x) x))]
  (assert (not ok?) "structural no-match raises")
  (assert (= (get err :error) :match-error) "structural error kind")
  (assert (= (get err :value) (list 1 2)) "structural error carries the value"))

# the error message names the match failure
(let [[ok? err] (protect (match 7
                           1 :one))]
  (assert (not ok?) "message test raises")
  (assert (string/contains? (get err :message) "match") "message mentions match"))
