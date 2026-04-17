# Syntax

Elle is a Lisp. Expressions are parenthesized, prefix-notation forms.
This document covers literal syntax and reader-level constructs.

## Immediates

```lisp
nil                  # absence of value (falsy)
true  false          # booleans (not #t/#f)
42                   # integer (64-bit signed)
3.14                 # float (64-bit IEEE 754)
-7                   # negative integer
0xFF                 # hexadecimal (255)
0o755                # octal (493)
0b1010               # binary (10)
1_000_000            # underscores are whitespace in numbers
```

## Keywords and symbols

Keywords are self-evaluating and interned. Symbols name bindings.
Both convert to the same string:

```lisp
(type-of :foo)       # => :keyword
(type-of 'foo)       # => :symbol

# a keyword and a symbol with the same name are not equal
(= 'name :name)      # => false

# but they share the same string representation
(assert (= (string :keyword) (string 'keyword) "keyword"))
```

## Empty list vs nil

`()` is the empty list — it is truthy. `nil` is the absence of a value
and is falsy. Lists terminate with `()`, not `nil`. Use `empty?` to
test for end-of-list; `nil?` will not work.

```lisp
(empty? (list))              # => true
(nil? (list))                # => false
(if (list) :truthy :falsy)   # => :truthy
(if nil :truthy :falsy)      # => :falsy
```

## Comments

`#` starts a line comment. Everything from `#` to end-of-line is ignored.
This is not Scheme — `;` is the splice operator, not a comment character.

## Splice

`;expr` splices a sequence into the surrounding form. Works in function
calls and collection literals.

```lisp
[1 ;[2 3] 4]        # => [1 2 3 4]
[;[1] ;[2] ;[3]]    # => [1 2 3]

(defn add3 [a b c] (+ a b c))
(add3 ;[1 2 3])     # => 6
```

## Quoting

`'expr` quotes — prevents evaluation, returning the form as data.

```lisp
(type-of '(+ 1 2))  # => :list (quoted list is data, not a call)
(first '(a b c))    # => a
```

Quasiquote (`` ` ``) builds templates. `,expr` unquotes (evaluates one
subexpression). `,;expr` unquote-splices (evaluates and spreads).

```lisp
(let [x 10]
  `(a ,x b))        # => (a 10 b)

(let [items '(2 3 4)]
  `(1 ,;items 5))   # => (1 2 3 4 5)
```

## Collection literals

Bare forms are immutable. `@`-prefixed forms are mutable.

```text
Syntax        Type       Mutable?
──────────────────────────────────
[1 2 3]       array      no
@[1 2 3]      @array     yes
{:a 1}        struct     no
@{:a 1}       @struct    yes
"hello"       string     no
@"hello"      @string    yes
|1 2 3|       set        no
@|1 2 3|      @set       yes
b[1 2 3]      bytes      no
@b[1 2 3]     @bytes     yes
```

```lisp
# immutable types
(type-of [1 2 3])    # => :array
(type-of {:a 1})     # => :struct
(type-of "hello")    # => :string
(type-of |1 2 3|)    # => :set

# mutable types
(type-of @[1 2 3])   # => :@array
(type-of @{:a 1})    # => :@struct
(type-of @"hello")   # => :@string
(type-of @|1 2 3|)   # => :@set
```

## Truthiness

Only `nil` and `false` are falsy. Everything else is truthy, including
`0`, `""`, `()`, and `[]`.

```lisp
(if 0 :yes :no)           # => :yes
(if "" :yes :no)           # => :yes
(if [] :yes :no)           # => :yes
(if (list) :yes :no)       # => :yes
(if nil :yes :no)          # => :no
(if false :yes :no)        # => :no
```

---

## See also

- [types.md](types.md) — type system and type predicates
- [macros.md](macros.md) — macro expansion and syntax objects
- [warts.md](warts.md) — intentional differences from other Lisps
