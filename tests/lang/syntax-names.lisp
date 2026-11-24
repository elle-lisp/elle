(elle/epoch 18)
# audited: 2026-11-24
# Every syntax operation is named under syntax/, no operation has a second name, and an older file reads the old names.
# docs/macros.md

# ── each operation, by its name ──────────────────────────────────────

(defmacro head-symbol? (form)
  (syntax/symbol? (syntax/first form)))
(assert (head-symbol? (f 1))
        "syntax/symbol? and syntax/first read a call's head")

(defmacro rest-length (form)
  (length (syntax/->list (syntax/rest form))))
(assert (= (rest-length (a b c)) 2)
        "syntax/rest and syntax/->list read the tail")

(defmacro shapes (form)
  (let [[a b c d] (syntax/->list form)]
    [(syntax/pair? a) (syntax/list? b) (syntax/keyword? c) (syntax/nil? d)]))
(assert (= (shapes ((x) () :k nil)) [true true true true])
        "the four kind predicates read the children of a list")

(defmacro first-value (form)
  (syntax/e (syntax/first form)))
(assert (= (first-value (42 x)) 42) "syntax/e unwraps an atom")

(assert (= (syntax/->datum (syntax/<-datum nil '(a b))) '(a b))
        "syntax/<-datum and syntax/->datum carry a datum there and back")

# ── no second name ───────────────────────────────────────────────────

(def old-names
  ['syntax->datum 'datum->syntax 'syntax-e 'syntax-pair? 'syntax-list?
   'syntax-symbol? 'syntax-keyword? 'syntax-nil? 'syntax->list 'syntax-first
   'syntax-rest 'meta/syntax->datum 'meta/datum->syntax 'meta/syntax-e
   'meta/syntax-pair? 'meta/syntax-list? 'meta/syntax-symbol?
   'meta/syntax-keyword? 'meta/syntax-nil? 'meta/syntax->list 'meta/syntax-first
   'meta/syntax-rest])

(each old in old-names
  (assert (not (first (protect (eval old)))) (string old " has no binding")))

# ── an older file ────────────────────────────────────────────────────

(defn migrated [source]
  "The forms SOURCE holds, migrated from the epoch it declares, as data."
  (map syntax/->datum (compile/read-forms source "<test>")))

(assert (= (migrated "(elle/epoch 18)\n(meta/syntax-e (syntax-first x))")
           '((syntax/e (syntax/first x))))
        "an epoch 18 file reads the bare and the meta/ names")

(def old-macro
  "(elle/epoch 18)
   (defmacro first-sym? (form) (syntax-symbol? (syntax-first form)))
   (first-sym? (f 1))")
(assert (= ((get (get (compile/whole-module old-macro "<test>") 0) 1)) true)
        "a macro an epoch 18 file defines still expands")
