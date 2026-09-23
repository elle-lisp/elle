# Macros

<!-- audited: 2026-09-22 -->

Elle's macros run as ordinary Elle code at expansion time, and are hygienic by
sets of scopes; `datum->syntax` breaks hygiene on purpose.

## Defining a macro

A macro is a name, a parameter list and a body. At expansion time the arguments
arrive unevaluated, the body runs in the VM, and the syntax it returns replaces
the call. The whole language is available in a macro body: `if`, `let`,
closures, recursion. The body usually builds its result with quasiquote.

```lisp
(defmacro my-when (test & body)
  `(if ,test (begin ,;body) nil))

(assert (= (my-when true 1 2) 2))
(assert (= (expand-macro '(my-when ok (f))) '(if ok (begin (f)) nil)))
```

- **`define-macro`** is an alias for `defmacro`.
- **Parameters** take the same `&opt` and `&` rest markers as a function.
- **A wrong argument count** is a compile error.
- **Expansion recurses**, so a macro may expand to another macro call. The depth
  is limited to 200, which stops an expansion that never ends.
- **`(macro? name)`** answers whether `name` names a macro, at expansion time.
- **`(expand-macro 'form)`** expands a quoted form and returns the result as
  data. Write the argument with `'`: a written-out `(quote form)` comes back
  unexpanded (#1229).
- **A `defmacro` form expands to `nil`**, so a definition inside a `begin`
  produces no code.

Two limits, both listed in [warts.md](warts.md): a macro cannot return an
improper list such as `(pair 1 2)`, and a macro cannot be exported from a
module.

## Hygiene: sets of scopes

A name a macro introduces cannot capture a name at its call site, and a call
site cannot capture a name the macro's template uses. Elle implements Racket's
sets-of-scopes model (Matthew Flatt, 2016): every identifier carries a set of
scopes, and each expansion mints a fresh *intro* scope.

```lisp
(defmacro my-swap (a b)
  `(let [tmp ,a] (assign ,a ,b) (assign ,b tmp)))

(def @tmp 100)
(def @x 1)
(def @y 2)
(my-swap x y)
(assert (= [x y tmp] [2 1 100]))   # the macro's tmp is not the caller's
```

The expansion stamps the intro scope on the arguments, then flips it on the
result. Identifiers that came from the template gain the scope; identifiers that
came from the arguments lose it again, and so keep their call-site scope sets.

A binding is visible to a reference when the binding's scope set is a subset of
the reference's. When several bindings match, the one with the largest set
wins. In the swap above, with call-site scope `{0}` and intro scope `3`:

| Reference | Candidate binding | Subset? |
|-----------|-------------------|---------|
| template `tmp`, scopes `{0, 3}` | caller's `tmp`, `{0}` | yes |
| template `tmp`, scopes `{0, 3}` | macro's `tmp`, `{0, 3}` | yes, and larger, so it wins |
| caller's `tmp`, scopes `{0}` | macro's `tmp`, `{0, 3}` | no, so it is invisible |

**Referential transparency.** A free name in a template resolves where the macro
was defined, not where it is called. Inside a local frame, a binding is visible
to a template reference only if it carries that reference's intro scope. A
call-site `let` therefore cannot shadow a name the template uses:

```lisp
(defn helper [x] (* x 10))
(defmacro scaled (e) `(helper ,e))

(let [helper (fn [x] :shadowed)]
  (assert (= (scaled 2) 20)))   # the template's helper is the top-level one
```

`gensym` is still available for a unique name that has nothing to do with
hygiene, such as a generated global.

## Breaking hygiene on purpose: `datum->syntax`

`(datum->syntax context datum)` builds a syntax object that carries
`context`'s scopes, and the intro scope is never stamped on it. It therefore
binds and resolves at the call site, which is what an anaphoric macro needs:

```lisp
(defmacro aif (test then else)
  `(let [,(datum->syntax test 'it) ,test]
     (if ,(datum->syntax test 'it) ,then ,else)))

(assert (= (aif (+ 1 2) (+ it 10) 0) 13))
```

When `context` is a plain value rather than a syntax object (an atom argument
arrives as a plain value), the result gets empty scopes, and ordinary lexical
scoping applies. `(syntax->datum stx)` strips the scopes and returns the plain
value.

## Pattern matching on syntax: `syntax-case`

`syntax-case` matches a syntax object against patterns: `_`, a pattern
variable, a literal, `(literal sym)` for a symbol, or a list of patterns. A
clause may carry a `when` guard.

```lisp
(defmacro classify (stx)
  (syntax-case stx
    ((literal if) :an-if)
    (42 :forty-two)
    ((a b) :a-pair)
    (_ :other)))

(assert (= (classify if) :an-if))
(assert (= (classify 42) :forty-two))
(assert (= (classify (x y)) :a-pair))
(assert (= (classify z) :other))
```

## Macros in the prelude

[src/prelude.lisp](../src/prelude.lisp) defines the everyday forms as ordinary macros:

| Macro | Purpose |
|-------|---------|
| `defn`, `let*` | Function definition; sequential `let` alias |
| `when`, `unless`, `case` | Conditionals |
| `if-let`, `when-let`, `when-ok` | Conditional binding |
| `try`/`catch`, `protect`, `defer`, `with` | Errors and cleanup |
| `each`, `forever`, `repeat` | Loops |
| `->`, `->>`, `as->`, `some->` | Threading |
| `apply` | Spread the final argument |
| `yield*` | Delegate to a sub-fiber, yielding its values |
| `ffi/defbind`, `ffi/with-stack` | FFI convenience |

`match`, `cond`, `if` and `while` are special forms, not macros.

## Implementation

[src/syntax/expand/AGENTS.md](../src/syntax/expand/AGENTS.md) documents the
expander: its dispatch order, argument wrapping, the transformer cache, and its
invariants. [impl/syntax.md](impl/syntax.md) documents the syntax tree the
expander rewrites.
