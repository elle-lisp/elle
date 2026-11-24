# Macros: syntax as data

<!-- audited: 2026-11-24 -->

A transformer reads its arguments as syntax objects: each node's kind, children, span and binding, through the `syntax/` operations.

[overview.md](overview.md) shows what a transformer cannot read today, and
[plan.md](plan.md) the steps this part lands in. [macros.md](../macros.md)
states how a macro expands.

## What a transformer receives

Every argument reaches a transformer as a syntax object: a symbol, a compound
form, and an atom such as a number, a string, a keyword, `nil` or a boolean.
Each one carries a span and a scope set, so a compile error can point at any
argument, and `syntax/<-datum` takes its context from any of them.

A syntax object is truthy, so a body that tests a literal argument unwraps it
first with `syntax/e`. An `&opt` parameter that the call leaves out stays plain
`nil`. A file of epoch 18 or older receives the same objects, because no rule
can find a body that tests an atom argument.

```lisp
(defmacro arg-kind (x) [(type-of x) (syntax/kind x)])
(assert (= [(arg-kind 1) (arg-kind "s") (arg-kind false)]
           [[:syntax :integer] [:syntax :string] [:syntax :boolean]]))

(defmacro switch (flag) (if (syntax/e flag) :on :off))
(assert (= [(switch true) (switch false) (switch nil)] [:on :off :off]))

(defmacro left-out? (&opt x) (nil? x))
(assert (left-out?) "a parameter the call leaves out stays nil")

(defmacro line-of (x) (get (syntax/span x) :line))
(let [here (meta/location)] (assert (= (line-of 7) (get here :line))))
```

## Reading a node

| Operation | Answers |
|-----------|---------|
| `(syntax/kind stx)` | A keyword for the node's kind: the name `type-of` gives the value the node reads as, or `:quote`, `:quasiquote`, `:unquote`, `:unquote-splicing` or `:splice` |
| `(syntax/children stx)` | An array of the node's children, each a syntax object; empty for an atom |
| `(syntax/rebuild stx children)` | A node of the same kind, span and scopes over the children an array or a list holds |
| `(syntax/span stx)` | `{:file :line :col}`, as `meta/location` answers for a form |
| `(syntax/error stx message)` | No value: it signals a `:syntax-error` that holds the node |

Each operation takes a syntax object, and signals `:type-error` for any other
value. A struct's children are its keys and values, in the order the source
writes them. A quote's one child is the form it quotes, so a quoting form
rebuilds over exactly one child, and an atom over none. Another count signals
`:arity-error`.

```lisp
(defmacro kinds (& forms) (list 'quote (map syntax/kind forms)))
(assert (= (kinds x 1 (f 1) [1] {:a 1} 'q `q) '(:symbol :integer :list :array :struct :quote :quasiquote)))

(defmacro width (form) (length (syntax/children form)))
(assert (= [(width {:a 1 :b 2}) (width 'q) (width x)] [4 1 0]))

(defmacro backwards (form)
  (syntax/rebuild form (reverse (syntax/children form))))
(assert (= (backwards [1 2 3]) [3 2 1]))
(assert (= (backwards (3 2 -)) -1))

(defmacro whereabouts (form)
  (let [s (syntax/span form)] [(get s :file) (get s :line)]))
(let [here (meta/location)] (assert (= (whereabouts (f)) [(get here :file) (get here :line)])))

(assert (= (get (get (protect (syntax/kind 5)) 1) :error) :type-error))
```

The kind and the children are enough to walk any tree without a case for each
kind, and `syntax/rebuild` puts a walked tree back.

A child that is not a syntax object converts as a transformer's result
converts. It has no scopes, so the expansion marks it as the template's own,
and no local binding at the call site captures it. A primitive or a standard
library function is visible to it. `syntax/<-datum` would exempt it from
hygiene instead.

```lisp
(defmacro call-inc (form) (syntax/rebuild form ['inc form]))
(let [inc (fn [x] :captured)]
  (assert (= (call-inc (+ 1 2)) 4) "the global inc, not the caller's"))
```

A `:syntax-error` that leaves a transformer fails the compile at the node's
span, and the message names the macro. A transformer that catches one reads
the node under `:syntax`.

```lisp
(def src "(defmacro named [form]\n  (if (syntax/symbol? form) form (syntax/error form \"wants a name\")))\n(named (f 1))")
(let [[ok? err] (protect (compile/whole-module src "<doc>"))]
  (assert (not ok?))
  (assert (string/contains? (get err :message) "<doc>:3:8: macro 'named': wants a name")))

(defmacro caught (form)
  (let [[_ err] (protect (syntax/error form "no"))]
    [(get err :error) (syntax/symbol? (get err :syntax))]))
(assert (= (caught x) [:syntax-error true]))
```

## A datum keeps its kind

`syntax/->datum` strips the scopes and answers the data `read` answers for the
same text ([syntax.md](../syntax.md)). A struct literal comes back as a struct,
and a set literal as a set. `syntax/<-datum` over the result builds a node of
the same kind, so a list written as `(struct :a b)` stays a list.

```lisp
(defmacro datum-type (x) (type-of (syntax/->datum x)))
(assert (= [(datum-type {:a b}) (datum-type |a|) (datum-type (struct :a b))]
           [:struct :set :list]))

(def keyed (first (compile/read-forms "{(f x) 1}" "<doc>")))
(assert (= (syntax/kind (syntax/<-datum nil (syntax/->datum keyed))) :struct)
        "a key that is code survives the round trip")
```

A transformer that must see every child in the order the source writes it
reads `syntax/children`, which keeps a repeated key.

## The names

Every syntax operation is named under `syntax/`, as the string operations are
named under `string/`. A name keeps the stem Racket gives the operation, so
`syntax->datum` is `syntax/->datum` and `datum->syntax` is `syntax/<-datum`. No
operation has a second name, and `(doc name)` states what each one does.

```lisp
(defmacro head-is-symbol? (form) (syntax/symbol? (syntax/first form)))
(assert (head-is-symbol? (f 1)))
(assert (= (syntax/->datum (syntax/<-datum nil '(a b))) '(a b)))
(assert (not (first (protect (eval 'syntax->datum)))) "the old name is gone")
```

A file of epoch 18 or older spells each operation with the name it had then:
`syntax->datum`, `datum->syntax`, `syntax-e`, or the name with `meta/` in
front. Migration renames it ([epochs.md](../epochs.md)):

```lisp
(assert (= (map syntax/->datum
                (compile/read-forms "(elle/epoch 18)\n(datum->syntax x (syntax-e y))" "<doc>"))
           '((syntax/<-datum x (syntax/e y)))))
```

## Pattern matching on syntax: `syntax-case`

`syntax-case` matches a syntax object against patterns: `_`, a pattern
variable, a literal, `(literal sym)` for a symbol, or a list of patterns. A
literal matches an atom node of its value, at the top or inside a list. A
clause may carry a `when` guard.

```lisp
(defmacro classify (stx)
  (syntax-case stx
    ((literal if) :an-if)
    (42 :forty-two)
    ((a 42) :ends-in-forty-two)
    ((a b) :a-pair)
    (_ :other)))

(assert (= (classify if) :an-if))
(assert (= (classify 42) :forty-two))
(assert (= (classify (x 42)) :ends-in-forty-two))
(assert (= (classify (x y)) :a-pair))
(assert (= (classify z) :other))
```

## Destructuring a node with `match`

`(syntax/e stx)` answers one level of a node as data. An atom node answers its
value. A compound node answers the collection `read` gives for it, holding each
child as a syntax object, and a quoting node answers a list such as `(quote x)`.

```lisp
(defmacro view-type (form) (type-of (syntax/e form)))
(assert (= [(view-type (a b)) (view-type [a]) (view-type {:a 1}) (view-type 'q)]
           [:list :array :struct :list]))
```

`match` reads a syntax object as `syntax/e` answers it, wherever a pattern
tests a value or steps into it. A variable binds what its place holds before
that read, so a name bound to a node holds the node, with its span and scopes.
Any other value matches as [match.md](../match.md) states.

```lisp
(defmacro swap-args (call)
  (match call
    (f a b) `(,f ,b ,a)
    _ (syntax/error call "wants a call of two arguments")))
(assert (= (swap-args (- 1 10)) 9))

(defmacro is-zero (x) (match x 0 :zero _ :other))
(assert (= [(is-zero 0) (is-zero 1) (is-zero (f))] [:zero :other :other]))

(defmacro head-kind (form) (match form (h & _) (syntax/kind h)))
(assert (= (head-kind ((f) 1)) :list))

(defmacro radius (shape) (match shape {:r r} r))
(assert (= (radius {:x 0 :r 5}) 5))
```

A `&` rest in a list pattern binds a list of the remaining children, each a
syntax object. The elements of a list a transformer already holds, such as a
`&` parameter, read the same way.

```lisp
(defmacro arg-count (form) (match form (_ & args) (length args)))
(assert (= (arg-count (f 1 2 3)) 3))

(defmacro first-name (& forms)
  (match forms
    ([name _ & _] & _) (list 'quote name)))
(assert (= (first-name [x 1] [y 2]) 'x))
```

A quoted symbol in a pattern matches a symbol node by its name, not by the
binding the name resolves to:

```lisp
(defmacro an-if? (form)
  (match form
    ('if & _) true
    _ false))
(assert (an-if? (if a b c)))
(assert (not (an-if? (cond a b))))
```

The decision tree takes a view step before each test and before each step into
a child. The step answers `syntax/e` of a syntax object and any other value
unchanged, so a match over data pays one tag test at each step. A view lives in
the region of the node it reads. A name bound through a view therefore borrows
the scrutinee, as a name bound to a list element does, and the view needs no
release of its own.

## Rejected alternatives

**Atoms stay plain values.** No macro body would change. An atom would carry
no span and no scopes, and every test of a kind would need a branch for a value
and a branch for an object.

**A predicate for each kind.** Five exist today. A walk over them needs an arm
for each, and a kind added later reaches no walker.

**The `meta/` prefix.** It already holds `meta/location` and `meta/gensym`,
which read no syntax object.

**A repeated key signals an error.** The value would then hold the literal
exactly, but `read` would refuse a file that compiles and runs.

**`read` keeps the call.** A tool that walks the forms `read` answers would see
another shape than a macro sees for the same text.

**A rebuilt child must be a syntax object.** A walk that puts a computed child
in place would then wrap it with `syntax/<-datum`, which exempts it from
hygiene.

**Each test and access instruction reads a syntax object.** Every backend
would learn the syntax object for a dozen instructions, where one view step
serves them all.

**A view in a region of its own.** The step builds a view only for a syntax
object, so a release of that region would need a route a match over data
skips.

**`syntax-case` takes the patterns of `match`.** A transformer would then learn
a second form for the patterns it already writes over data.
