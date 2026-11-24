# Macros: the design

<!-- audited: 2026-11-24 -->

Where macros are going: a macro is a lexical binding and a sealed value, the analyzer expands it, and the fold answers questions of type.

[macros.md](../macros.md) states how a macro expands today. The documents
beside this one state the design it becomes, and none of that design is built.
[plan.md](plan.md) states the order it lands in and the decisions still open.

## Where the macro system stands

The expander finds a macro by the spelling of a form's head, in one table for
the whole unit. It walks every list the same way, so it expands a pattern and a
parameter list as code. Each claim below runs as written:

```lisp
# A local binding does not shadow a macro.
(assert (= 2 (let [when (fn [a b] :local)] (when 1 2))))

# A pattern and a parameter list are expanded as code.
(assert (= :no (match (list 1 2) (when b) :matched _ :no)))
(assert (not (first (protect (eval '((fn (when x) x) 1 2))))))

# A macro and a variable of one name are two things.
(assert (= [1 2] (eval '(begin (def m 1) (defmacro m [] 2) [m (m)]))))

# A definition in a function body reaches the code after that function.
(assert (= 1 (eval '(begin (defn f [] (defmacro inner [] 1) (inner)) (inner)))))

# An atom argument is a plain value, and a symbol argument is not its datum.
(defmacro argument-type (x) (list 'quote (type-of x)))
(assert (= [:syntax :integer] [(argument-type foo) (argument-type 1)]))
(defmacro is-foo? (x) (= x 'foo))
(assert (not (is-foo? foo)))

# A struct literal does not survive the trip to a datum.
(defmacro as-datum (x) (list 'quote (syntax/->datum x)))
(assert (= '(struct :a b) (as-datum {:a b})))

# A user macro replaces one that a prelude template uses.
(assert (= :hijacked
           (eval '(begin (defmacro if-let (& xs) :hijacked) (when-let [x 1] x)))))
```

An expansion reclaims every heap value it built when it ends. A value it
stored in a `begin-for-syntax` table goes too, and a later expansion reads
that value (#1557).

## The rule

A macro reads what is settled before it runs, and it checks what settles after
it. An expansion happens at one point of one pass over the unit, so the rule
sorts every fact a macro could want:

| Fact | Settled | What a macro does |
|------|---------|-------------------|
| The binding an identifier resolves to | before | reads it ([binding.md](binding.md), [syntax.md](syntax.md)) |
| The value of a static binding | before | reads it ([static.md](static.md)) |
| The special forms a subform expands to | before | reads them ([walks.md](walks.md)) |
| An inferred type or signal | after, over the whole unit | writes the question into the program ([types.md](types.md)) |

## Terms

| Term | Meaning |
|------|---------|
| Head | The first element of a list form |
| Static | A binding is static when the compile holds its value |
| Sealed | Immutable all the way down, as [sealing.md](../impl/image/sealing.md) defines it for an image |
| Place | A template places an argument where it puts the argument in a position that runs |

## Expansion stays pure

A transformer stays deterministic and contained, as
[compile.md](../modules/compile.md) states. This design depends on that rule
more than the module system does. A static value becomes a constant of the
unit, so the unit's inputs must fix it. A body walk expands a subform that the
analyzer expands again, so both expansions must agree.
