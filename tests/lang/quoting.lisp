(elle/epoch 19)
# audited: 2026-11-24
# A quoting form means one thing in either spelling, a quasiquote outside a macro builds data, and ,; spreads any sequence.
# docs/syntax.md
# docs/macros.md

## ── one form, two spellings ───────────────────────────────────────

# A written list reaches a macro as the node its prefix builds.
(defmacro kind-of (form)
  (syntax/kind form))
(assert (= [(kind-of (quote x)) (kind-of (quasiquote x)) (kind-of (unquote x))
            (kind-of (unquote-splicing x))]
           [:quote :quasiquote :unquote :unquote-splicing]))
(assert (= (kind-of (quote a b)) :list) "a quote holds exactly one form")

# A value converts the same way, as a macro's result does.
(assert (= (syntax/kind (syntax/<-datum nil '(quote x))) :quote))
(assert (= (syntax/kind (syntax/<-datum nil '(unquote-splicing x)))
           :unquote-splicing))

# So does a rebuilt node.
(defmacro requote (form)
  (syntax/rebuild form ['quote (get (syntax/children form) 1)]))
(assert (= (requote (f (when true 1))) '(when true 1))
        "a rebuilt quote stays data")

# `read` answers the list for either spelling.
(assert (= (read "'x") '(quote x)))
(assert (= (read "(quote x)") '(quote x)))

## ── a quoted form stays data ──────────────────────────────────────

(assert (= (eval '(quote (when true 1))) '(when true 1)))
(defmacro quoted-when []
  (list 'quote '(when true 1)))
(assert (= (quoted-when) '(when true 1)) "a quote a macro returns")

# The counter-factual: when the expander read the quote list as a call, it
# expanded `bad` while it expanded `outer`, outside the protect.
(assert (nil? (eval '(begin
                       (defmacro outer []
                         (begin
                           (protect (eval
                                      '(begin
                                         (defmacro bad []
                                           (error :boom))
                                         (bad))))
                           nil))
                       (outer))))
        "a macro call in quoted data runs when its own eval runs")

## ── expand-macro takes either spelling ────────────────────────────

(defmacro twice (x)
  `(begin
     ,x
     ,x))
(assert (= (expand-macro '(twice 5))
           '(begin
              5
              5)))
(assert (= (expand-macro (quote (twice 5)))
           '(begin
              5
              5)))
(let [[ok? err] (protect (compile/whole-module
                           "(defmacro tw (x) x)\n(expand-macro (tw 5))" "<test>"))]
  (assert (not ok?) "an argument that is no quoted form does not compile")
  (assert (string/contains? (get err :message) "expand-macro")))

## ── a quasiquote in either spelling, and under eval ───────────────

(assert (= (quasiquote (a (unquote (+ 1 2)))) '(a 3)))
(assert (= (eval '`(a ,(+ 1 2))) '(a 3)))
(eval '(defmacro inc1 [x]
         `(+ ,x 1)))
(assert (= (eval '(inc1 2)) 3))

# An inner level stays a quasiquote in the value.
(let [n 5]
  (assert (= `(a `(b ,,n)) '(a `(b ,5)))))

# A macro defines a macro whose template is a quasiquote.
(defmacro def-adder [name n]
  `(defmacro ,name [x]
     `(+ ,x ,,n)))
(def-adder add5 5)
(assert (= (add5 1) 6))

# An unquote-splice at the inner level stays one for the defined macro.
(defmacro def-seq [name]
  `(defmacro ,name [& forms]
     `(begin
        ,;forms)))
(def-seq my-begin)
(assert (= (my-begin 1 2 3) 3))

## ── a quasiquote outside a macro builds data ──────────────────────

(assert (= `(a b) '(a b)))
(assert (= (first `(a)) 'a))
(assert (= (type-of (first `(a))) :symbol))
(assert (= (string (first `(a))) "a"))
(assert (= (eval `(+ ,1 2)) 3))

## ── a quasiquote in a transformer builds code ─────────────────────

# A template name resolves where the template was written, so a caller's
# local of that name does not capture it.
(defn helper10 [x]
  (* x 10))
(defmacro scaled (e)
  `(helper10 ,e))
(let [helper10 (fn [x] :shadowed)]
  (assert (= (scaled 2) 20)))

(begin-for-syntax (def wrap-inc (fn [x] `(+ ,x 1))))
(defmacro via-helper [x]
  (wrap-inc x))
(assert (= (via-helper 2) 3) "a template in a begin-for-syntax definition")

## ── unquote-splicing spreads any sequence ─────────────────────────

(def xs [1 2])
(assert (= `(a ,;xs) '(a 1 2)) "an array")
(assert (= `(a ,;@[1 2] b) '(a 1 2 b)) "an @array")
(assert (= `(a ,;(list 1 2)) '(a 1 2)) "a list")
(assert (= `(,;[]) ()) "an empty array")
(assert (= `[a ,;xs] ['a 1 2]) "into an array template")

# syntax/children answers an array, which a template spreads directly.
(defmacro sum-of [arr]
  `(+ ,;(syntax/children arr)))
(assert (= (sum-of [1 2 3]) 6))

## ── an unquote outside a quasiquote ───────────────────────────────

(defn compiles? [src]
  (first (protect (compile/whole-module src "<test>"))))
(assert (not (compiles? "(def x 1)\n,x")))
(assert (not (compiles? "(def x 1)\n(unquote x)")))
