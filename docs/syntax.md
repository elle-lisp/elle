# Syntax

<!-- audited: 2026-09-23 -->

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
[epochs.md](epochs.md) describes the change, and how `elle rewrite` migrates
such a file.

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
