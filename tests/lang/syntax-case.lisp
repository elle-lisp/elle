(elle/epoch 19)
# audited: 2026-11-24
# syntax-case picks the first clause whose pattern, and guard if any, matches a syntax object, and binds its pattern variables.
# docs/macros/syntax.md

# ── Patterns ─────────────────────────────────────────────────────────

(defmacro sc-wild [stx]
  (syntax-case stx (_ :matched)))
(assert (= (sc-wild 42) :matched) "a wildcard matches an int")
(assert (= (sc-wild :foo) :matched) "a wildcard matches a keyword")
(assert (= (sc-wild (a b)) :matched) "a wildcard matches a list")

(defmacro sc-var [stx]
  (syntax-case stx (x (syntax/->datum x))))
(assert (= (sc-var 99) 99) "a pattern variable binds an int")
(assert (= (sc-var :kw) :kw) "a pattern variable binds a keyword")

(defmacro sc-int [stx]
  (syntax-case stx (42 :forty-two) (_ :other)))
(assert (= (sc-int 42) :forty-two) "a literal int matches itself")
(assert (= (sc-int 43) :other) "a literal int matches no other int")

(defmacro sc-kw [stx]
  (syntax-case stx (:yes :found-yes) (_ :other)))
(assert (= (sc-kw :yes) :found-yes) "a literal keyword matches itself")
(assert (= (sc-kw :no) :other) "a literal keyword matches no other keyword")

(defmacro sc-sym [stx]
  (syntax-case stx ((literal if) :found-if) (_ :other)))
(assert (= (sc-sym if) :found-if) "(literal if) matches the symbol if")
(assert (= (sc-sym foo) :other) "(literal if) matches no other symbol")

(defmacro sc-pair [stx]
  (syntax-case stx ((a b) (syntax/->datum a)) (_ :not-pair)))
(assert (= (sc-pair (1 2)) 1) "a list pattern binds each element")
(assert (= (sc-pair (1)) :not-pair) "a list pattern needs its exact length")

(defmacro sc-empty [stx]
  (syntax-case stx (() :empty) (_ :nonempty)))
(assert (= (sc-empty ()) :empty) "() matches the empty list")
(assert (= (sc-empty (a)) :nonempty) "() matches no other list")

(defmacro sc-exact [stx]
  (syntax-case stx (42 :found-42)))
(assert (= (sc-exact 42) :found-42) "a lone literal clause matches its value")

(defmacro sc-nil [stx]
  (syntax-case stx (nil :found-nil) (_ :other)))
(assert (= (sc-nil nil) :found-nil) "a literal nil matches nil")
(assert (= (sc-nil ()) :other) "a literal nil matches no empty list")
(assert (= (sc-nil false) :other) "a literal nil matches no false")

# Two counter-factuals. A test that reads an element through the name the
# clause binds it to runs before that binding exists, so the clause fails to
# compile. A literal compared with the element as it stands compares a syntax
# object with a plain value, so no element ever matches.
(defmacro sc-inner [stx]
  (syntax-case stx ((a 42) :ends-in-42) ((a :k) :ends-in-k) ((a "s") :ends-in-s)
               ((a false) :ends-in-false) ((a nil) :ends-in-nil)
               ((a (literal if)) :ends-in-if) (_ :other)))
(assert (= (sc-inner (x 42)) :ends-in-42) "a literal int matches an element")
(assert (= (sc-inner (x :k)) :ends-in-k) "a literal keyword matches an element")
(assert (= (sc-inner (x "s")) :ends-in-s) "a literal string matches an element")
(assert (= (sc-inner (x false)) :ends-in-false)
        "a literal false matches an element")
(assert (= (sc-inner (x nil)) :ends-in-nil) "a literal nil matches an element")
(assert (= (sc-inner (x if)) :ends-in-if) "(literal if) matches an element")
(assert (= (sc-inner (x 43)) :other) "a literal matches no other element")

(defmacro sc-nested [stx]
  (syntax-case stx ((a (b c)) (syntax/->datum c)) (_ :other)))
(assert (= (sc-nested (x (y 3))) 3) "a list pattern holds a list pattern")
(assert (= (sc-nested (x y)) :other) "a nested list pattern needs a list")
(assert (= (sc-nested (x (y))) :other)
        "a nested list pattern needs its exact length")

# ── Pattern variables are ordinary names ─────────────────────────────

# The trap: syntax-case binds its scrutinee under generated names that start
# with __sc. An expansion that told those apart from user names by prefix
# gave a user variable such as __scanner the generated names' scopes, and the
# body's reference to it failed to resolve.
(defmacro sc-prefix-var [stx]
  (syntax-case stx (__scanner (syntax/->datum __scanner))))
(assert (= (sc-prefix-var 99) 99)
        "a pattern variable named __scanner binds like any other")

(defmacro sc-prefix-list [stx]
  (syntax-case stx
               ((__sca __scb) (+ (syntax/->datum __sca) (syntax/->datum __scb)))
               (_ :not-pair)))
(assert (= (sc-prefix-list (40 2)) 42)
        "list-pattern variables with the __sc prefix bind")

# ── Guards and clause order ──────────────────────────────────────────

(defmacro sc-guard [stx]
  (syntax-case stx (x when (syntax/symbol? x) :got-symbol) (_ :other)))
(assert (= (sc-guard foo) :got-symbol) "a guard that holds selects its clause")
(assert (= (sc-guard 42) :other) "a guard that fails passes to the next clause")

(defmacro sc-multi [stx]
  (syntax-case stx (1 :one) (2 :two) (_ :other)))
(assert (= (sc-multi 1) :one) "the first matching clause wins")
(assert (= (sc-multi 2) :two) "a later clause matches when earlier ones fail")
(assert (= (sc-multi 99) :other) "a wildcard catches what no clause matched")

# ── In a macro with several parameters ───────────────────────────────

(defmacro my-if [test then else-expr]
  (syntax-case test (true (syntax/->datum then))
               (false (syntax/->datum else-expr))
               (_ (list 'if (syntax/->datum test) (syntax/->datum then)
                        (syntax/->datum else-expr)))))

(assert (= (my-if true 1 2) 1) "a literal true test selects then")
(assert (= (my-if false 1 2) 2) "a literal false test selects else")
(assert (= (my-if (= 1 1) 1 2) 1) "any other test expands to an if")
