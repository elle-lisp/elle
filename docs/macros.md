# Macros

<!-- audited: 2026-10-14 -->

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

## What a macro body may do

A macro body runs while a compile waits for it, and what it returns becomes
part of the unit. So the unit's own inputs decide what the body does. It may
raise, but it reads no state outside the unit and changes none:

- **A macro call runs under a fuel budget** ([runtime.md](runtime.md)). The
  budget counts silent code, the fibers the body resumes and the expansions it
  starts. A body that exhausts it fails the compile, and the error names the
  macro.
- **A body spends no effect capability.** Every expansion withholds
  `|:ffi :io :exec :gpu :os-signal :fs :net|`, beside what the fiber whose code
  started the compile withholds. A primitive that needs one is denied, and the
  compile fails naming the macro and the primitive
  ([capabilities.md](signals/capabilities.md)). So a body cannot print, read a
  file, open a socket or load a library.
- **A body reads no ambient state.** Some primitives reach state outside the
  unit through no capability bit: the environment, the arguments, the pid, the
  clocks, `vm/config`, the signal registry, `hash`, the standard ports, and
  the VM's caches and gauges. A call to one halts the expansion. The halt
  carries `:terminal`, so it passes every fiber mask, every `protect` and the
  boundary of every silent function. The compile fails naming the macro and
  the primitive. `(vm/primitive-meta name)`
  answers `:ambient true` for each, and `(doc name)` says so.
- **The primitives the compile binds are an input.** `vm/list-primitives`,
  `doc` and `vm/primitive-meta` answer for each in a body as they do at run
  time, a primitive the host registers included ([embedding.md](embedding.md)).
- **A plugin's primitive runs** unless it declares a capability the expansion
  withholds.
- **A macro defined at the REPL reaches later REPL lines only.** A file that a
  line imports expands against the prelude, its own macros and the macros of
  the files it includes.

An `include` reads its file as an input of the compile, not as an effect of a
macro, so every compile reads it. The fiber that started the compile still
needs `:fs` for it.

```lisp
(defn compile-error [form]
  (let [[ok? err] (protect (eval form))]
    (assert (not ok?) "the compile fails")
    (get err :message)))

(def spin '(begin (defmacro spin [] (forever nil)) (spin)))
(assert (string/contains? (compile-error spin) "macro 'spin'") "naming the macro")

(def home '(begin (defmacro home [] (protect (sys/env "HOME"))) (home)))
(let [msg (compile-error home)]
  (assert (string/contains? msg "macro 'home'") "naming the macro")
  (assert (string/contains? msg "sys/env") "and the primitive, past the protect"))

(def pid '(begin (defmacro pid []
                   (fiber/resume (fiber/new (fn [] (sys/pid)) |:halt :error|)))
                 (pid)))
(assert (string/contains? (compile-error pid) "sys/pid") "no fiber mask catches the halt")

(def shout '(begin (defmacro shout [] (println "expanding")) (shout)))
(assert (string/contains? (compile-error shout) "macro 'shout'") "printing is an effect")

(assert (get (vm/primitive-meta "sys/env") :ambient))
(assert (not (get (vm/primitive-meta "string/join") :ambient)))
```

The check at run time does not reach two reads. Under `eval`, a macro body
runs on the caller's fiber, so a parameter it calls answers the caller's
dynamic binding. A closure, a syntax object and a parameter order by their
address, so a macro that sorts them may expand differently in two processes
(#1387).

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

### An expansion depends on its unit alone

A unit is what one compile expands: a file, a REPL line, or the datum one
`eval` compiles. The scopes an expansion mints, and the names `gensym` answers
while it runs, carry the unit's tag and a count that starts at zero in the unit
([impl/syntax.md](impl/syntax.md)). A unit therefore expands to the same tree in
every process, whatever the process expanded before it, and two units mint
different names. Outside an expansion, `gensym` counts across the process, so
two calls at run time answer two symbols. A `begin-for-syntax` definition
belongs to the unit that makes it, so a later `eval` does not see one an
earlier `eval` made.

```lisp
(def fresh-name '(begin (defmacro fresh [] (list 'quote (gensym "t"))) (fresh)))
(def other-unit '(begin (defmacro other [] (list 'quote (gensym "t"))) (other)))
(assert (= (eval fresh-name) (eval fresh-name)) "one unit expands to one name")
(assert (not (= (eval fresh-name) (eval other-unit))) "two units, two names")
(assert (not (= (gensym "t") (gensym "t"))) "two calls at run time, two symbols")

(eval '(begin-for-syntax (def helper (fn [] 42))))
(eval '(defmacro uses-helper [] (helper)))
(let [[ok? _] (protect (eval '(uses-helper)))]
  (assert (not ok?) "a later eval does not see the helper"))
```

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
