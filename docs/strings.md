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
(string/find "hello" "ll")        # => 2 (index, or nil)
(string/contains? "hello" "ell")   # => true
(string/starts-with? "hello" "he") # => true
(string/ends-with? "hello" "lo")   # => true
```

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
```

## Mutable @strings

`@"..."` creates a mutable string. `get`, `put`, `length`, `push`, and
`pop` are all grapheme-indexed.

```lisp
(def s @"hello")
(get s 0)                          # => "h"
(put s 0 "H")                     # mutates; s is now @"Hello"
(push s "!")                       # mutates; s is now @"Hello!"
(pop s)                            # => "!" (removes and returns last)
```

## Conversion

```lisp
(thaw "hello")                     # => @"hello"
(freeze @"hello")                  # => "hello"
(string 42)                        # => "42"
(string :foo)                      # => "foo" (no colon)
```

---

## See also

- [bytes.md](bytes.md) — binary data
- [types.md](types.md) — type system and mutability
- [arrays.md](arrays.md) — array operations (string/split returns arrays)
