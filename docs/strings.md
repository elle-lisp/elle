# Strings

Strings are immutable sequences of grapheme clusters. All string operations
use the `string/` prefix. `@"..."` creates a mutable string.

## Access and length

```lisp
(length "hello")                   # => 5 (grapheme count)
(string/size-of "hello")           # => 5 (byte count)
(get "hello" 0)                    # => "h" (grapheme cluster)
(get "hello" -1)                   # => "o" (negative indexes)
(slice "hello" 1 4)                # => "ell"
```

Grapheme count and byte count differ for multi-byte characters:

```lisp
(length "👋🏽")                      # => 1 (one grapheme cluster)
(string/size-of "👋🏽")              # => 8 (4+4 bytes UTF-8)
```

## Concatenation

```lisp
# string converts all args to strings and concatenates
(string "hello " "world")          # => "hello world"
(string "count: " 42)              # => "count: 42"

# join a collection with a separator
(string/join ["a" "b" "c"] ",")    # => "a,b,c"

# format with placeholders
(string/format "{} + {} = {}" 1 2 3)  # => "1 + 2 = 3"
```

## Search and test

```lisp
(string/find "hello" "ll")        # => 2 (grapheme index, or nil if not found)
(string/find "hello" "xx")        # => nil
(string/find "hello" "lo" 3)      # => 3 (with optional start offset)
(string/index-of "hello" "ll")    # => 2 (alias for string/find)
(string/contains? "hello" "ell")   # => true
(string/starts-with? "hello" "he") # => true
(string/ends-with? "hello" "lo")   # => true
```

`string/find` returns the grapheme index of the first occurrence, or `nil`
if the substring is not found. An optional third argument sets the start
offset. `string/index-of` is an alias.

## Transformation

```lisp
(string/upcase "hello")            # => "HELLO"
(string/downcase "HELLO")          # => "hello"
(string/trim "  hi  ")             # => "hi"
(string/replace "foo-bar" "-" "_") # => "foo_bar"
(string/repeat "-" 20)             # => "--------------------"
```

## Splitting

```lisp
(string/split "a,b,c" ",")        # => ["a" "b" "c"] (returns array)
(string/split "one two three" " ") # => ["one" "two" "three"]
(string/split "a,,b" ",")         # => ["a" "" "b"] (empty strings between consecutive delimiters)
```

`string/split` returns an array of substrings. Consecutive delimiters
produce empty strings in the result. The delimiter cannot be empty.

## Mutable @strings

`@"..."` creates a mutable string. `get`, `put`, `length`, `push`, and
`pop` are all grapheme-indexed. `put` mutates in place and returns the
@string.

```lisp
(def s @"hello")
(get s 0)                          # => "h"
(put s 0 "H")                     # mutates and returns s; s is now @"Hello"
(push s "!")                       # mutates and returns s; s is now @"Hello!"
(pop s)                            # => "!" (removes and returns last)
```

## Conversion

```lisp
(thaw "hello")                     # => @"hello"
(freeze @"hello")                  # => "hello"
(string 42)                        # => "42"
(string :foo)                      # => "foo" (no colon)
```

## Unicode version

Grapheme cluster boundaries come from UAX #29. The Unicode Consortium
revises those rules between Unicode versions. Each elle build vendors one
or more table *generations*; every VM selects one generation at
construction and keeps it for its whole life. Mid-run switching does not
exist: text ports buffer bytes split at cluster boundaries, and changing
tables mid-stream would corrupt that framing.

`unicode!` is the compile-time surface. With no arguments, it is a query
that folds to the selected generation's version. With arguments, it is a
declaration checked by the compiler; like the other `!` forms, it emits
no runtime code and evaluates to `nil`.

```lisp
(unicode!)                         # => [17 0 0]
(unicode! 17)                      # accepts any 17.x.x, evaluates to nil
(unicode! 17 0)                    # accepts any 17.0.x
(vm/config :unicode)               # => [17 0 0] (runtime introspection)
```

Put the declaration at the top of a file whose logic depends on exact
cluster boundaries: emoji ZWJ sequences, Indic conjuncts, or line framing
over text ports.

The generation is selected before the VM exists, by three agreeing
surfaces: the `(unicode! …)` declaration in the main file, the
`--unicode=MAJ[.MIN[.PATCH]]` CLI flag, and the embedding constructor
`Runtime::with_unicode`. Absent all three, the newest vendored generation
is used. The surfaces must agree; a conflict is a startup error. After
selection, every `(unicode! …)` anywhere in the program — imports, `eval`,
the REPL — is an assertion against the locked generation, so a program
cannot mix cluster semantics.

Requesting a generation the build does not vendor is a compile error that
lists the vendored versions. Declaring a vendored generation actually
selects it: under `--unicode=16.0` (or a `(unicode! 16)` main file),
`length`, `get`, and `slice` follow the Unicode 16.0 tables, so source
keeps its cluster semantics when newer builds change the default.

---

## See also

- [bytes.md](bytes.md) — binary data
- [types.md](types.md) — type system and mutability
- [arrays.md](arrays.md) — array operations (string/split returns arrays)
