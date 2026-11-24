# Syntax

<!-- audited: 2026-11-24 -->

The literals and reader-level constructs of Elle source, from numbers and
string escapes to quoting and collections.

Elle is a Lisp. Expressions are parenthesized, prefix-notation forms.

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
1_000_000            # underscores separate digits (1000000)
```

## Strings

A string literal is text between double quotes. It holds valid UTF-8, and
its length counts grapheme clusters (see [strings.md](strings.md)). Every
character inside the quotes stands for itself, a newline included, except `"`
and `\`.

A backslash starts an escape:

| Escape | Character |
|--------|-----------|
| `\n` | line feed, U+000A |
| `\t` | tab, U+0009 |
| `\r` | carriage return, U+000D |
| `\\` | backslash |
| `\"` | double quote |
| `\0` | NUL, U+0000 |
| `\xHH` | U+00HH, for exactly two hex digits from `00` to `7f` |
| `\u{H}` to `\u{HHHHHH}` | the Unicode scalar value, in one to six hex digits |

```lisp
(assert (= "\x41" "A") "a hex escape names an ASCII character")
(assert (= "\u{e9}" "é") "a unicode escape names a scalar value")
(assert (= (length "\0") 1) "NUL is one character")
(assert (= (string/size-of "\u{1F600}") 4) "U+1F600 is four bytes of UTF-8")
```

Any other escape is a read error that names the escape. So is `\x80` and
above: a string holds characters, and `\x80` does not name one byte and one
character at the same time. Write `\u{80}` for the character U+0080, and use
[bytes](bytes.md) for raw bytes. `\u{d800}` is an error too, because a
surrogate is not a scalar value.

```lisp
(let [[ok? err] (protect (read "\"\\q\""))]
  (assert (not ok?) "an unknown escape does not read")
  (assert (has? (get err :message) "\\q") "the error names the escape"))
```

A file that declares epoch 12 or earlier reads only the first five escapes,
and drops the backslash of any other: `"\x41"` is the string `x41` there.
[The lexicon](impl/lexicon.md) says how `elle rewrite` migrates such a file.

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
See [empty-list.md](empty-list.md) for the full rationale.

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

A mutable literal builds a new value each time it runs, so a later run never
sees a write made into an earlier run's value. An immutable literal compares by
its contents ([types.md](types.md)), so a program cannot tell whether two runs
built one value or two.

```lisp
(def bufs @[])
(each k in [1 2 3]
  (let [b @"x"]
    (push b "y")
    (push bufs b)))
(each b in bufs
  (assert (= (freeze b) "xy") "each run of @\"x\" builds a new buffer"))
```

## Source as data

`read` and `read-all` turn source text into data, and `syntax/->datum` turns a
syntax object into the same data ([macros/syntax.md](macros/syntax.md)). A
literal reads as the value it writes: a string for `"foo"`, an array for
`[1 x]`, a struct for `{:a 1}` and a set for `|x y|`. The `@` forms read as
their mutable kinds. A part of a literal that is code stays code: `{:a (f x)}`
reads as a struct whose value is the list `(f x)`. `eval` runs that data as
the literal would run.

A bytes literal reads as a call of `bytes`, because its elements are
expressions and a bytes value holds only integers. A quote differs from
`read`: a quoted struct, set or bytes literal is a call, so `'{:a 1}` is the
list `(struct :a 1)`.

```lisp
(assert (= (read "{:a (f x)}") {:a '(f x)}) "a struct whose value is code")
(assert (= (type-of (read "@{:a 1}")) :@struct))
(assert (= (read "|x y|") |'x 'y|))
(assert (= (read "b[1 2]") '(bytes 1 2)) "a bytes literal is a call")
(assert (= '{:a 1} '(struct :a 1)) "a quote keeps the call")
(assert (= (eval (read "{:a (+ 1 2)}")) {:a 3}))
```

The value decides what the literal repeats. A repeated key keeps its last
value, as the running struct does, and a repeated set element is one element.
Keys and elements come in the value's order, not the order the source writes.
A key that no struct holds, a float or a mutable literal, signals
`:type-error`.

```lisp
(assert (= (read "{:a 1 :a 2}") {:a 2}) "the last value of a repeated key")
(assert (= (length (read "|(f) (f)|")) 1) "a repeated element is one")
(assert (= (get (get (protect (read "{1.5 :x}")) 1) :error) :type-error))
(assert (= (get (get (protect (read "{@[1] :x}")) 1) :error) :type-error))
```

`read` runs at run time, so a file of any epoch reads the same data.

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
- [macros.md](macros.md) — macro expansion and hygiene
- [macros/syntax.md](macros/syntax.md) — syntax objects
- [warts.md](warts.md) — intentional differences from other Lisps
