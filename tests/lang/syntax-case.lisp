(elle/epoch 14)
# audited: 2026-09-30
# syntax-case picks the first clause whose pattern, and guard if any, matches a syntax object, and binds its pattern variables.
# docs/macros.md

# ── Patterns ─────────────────────────────────────────────────────────

(defmacro sc-wild [stx]
  (syntax-case stx (_ :matched)))
(assert (= (sc-wild 42) :matched) "a wildcard matches an int")
(assert (= (sc-wild :foo) :matched) "a wildcard matches a keyword")
(assert (= (sc-wild (a b)) :matched) "a wildcard matches a list")

(defmacro sc-var [stx]
  (syntax-case stx (x (syntax->datum x))))
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
  (syntax-case stx ((a b) (syntax->datum a)) (_ :not-pair)))
(assert (= (sc-pair (1 2)) 1) "a list pattern binds each element")
(assert (= (sc-pair (1)) :not-pair) "a list pattern needs its exact length")

(defmacro sc-empty [stx]
  (syntax-case stx (() :empty) (_ :nonempty)))
(assert (= (sc-empty ()) :empty) "() matches the empty list")
(assert (= (sc-empty (a)) :nonempty) "() matches no other list")

(defmacro sc-exact [stx]
  (syntax-case stx (42 :found-42)))
(assert (= (sc-exact 42) :found-42) "a lone literal clause matches its value")

# ── Pattern variables are ordinary names ─────────────────────────────

# The trap: syntax-case binds its scrutinee under generated names that start
# with __sc. An expansion that told those apart from user names by prefix
# gave a user variable such as __scanner the generated names' scopes, and the
# body's reference to it failed to resolve.
(defmacro sc-prefix-var [stx]
  (syntax-case stx (__scanner (syntax->datum __scanner))))
(assert (= (sc-prefix-var 99) 99)
        "a pattern variable named __scanner binds like any other")

(defmacro sc-prefix-list [stx]
  (syntax-case stx
               ((__sca __scb) (+ (syntax->datum __sca) (syntax->datum __scb)))
               (_ :not-pair)))
(assert (= (sc-prefix-list (40 2)) 42)
        "list-pattern variables with the __sc prefix bind")

# ── Guards and clause order ──────────────────────────────────────────

(defmacro sc-guard [stx]
  (syntax-case stx (x when (syntax-symbol? x) :got-symbol) (_ :other)))
(assert (= (sc-guard foo) :got-symbol) "a guard that holds selects its clause")
(assert (= (sc-guard 42) :other) "a guard that fails passes to the next clause")

(defmacro sc-multi [stx]
  (syntax-case stx (1 :one) (2 :two) (_ :other)))
(assert (= (sc-multi 1) :one) "the first matching clause wins")
(assert (= (sc-multi 2) :two) "a later clause matches when earlier ones fail")
(assert (= (sc-multi 99) :other) "a wildcard catches what no clause matched")

# ── In a macro with several parameters ───────────────────────────────

(defmacro my-if [test then else-expr]
  (syntax-case test (true (syntax->datum then))
               (false (syntax->datum else-expr))
               (_ (list 'if (syntax->datum test) (syntax->datum then)
                        (syntax->datum else-expr)))))

(assert (= (my-if true 1 2) 1) "a literal true test selects then")
(assert (= (my-if false 1 2) 2) "a literal false test selects else")
(assert (= (my-if (= 1 1) 1 2) 1) "any other test expands to an if")
