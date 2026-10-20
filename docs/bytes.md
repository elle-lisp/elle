# Bytes

<!-- audited: 2026-10-20 -->

Bytes are sequences of raw byte values (0–255). `b[...]` is immutable;
`@b[...]` is mutable.

## Literals

```lisp
b[1 2 3]                   # immutable bytes
@b[1 2 3]                  # mutable @bytes
b[0xff 0x0a 32]            # mixed hex and decimal
```

The `b[...]` syntax parallels `[...]` for arrays — `b` for bytes.

## Construction (function form)

The `bytes` and `@bytes` functions construct bytes:

```lisp
(bytes 1 2 3)              # immutable bytes from integers
(bytes "hello")            # string → bytes (UTF-8 encoding)
(@bytes 1 2 3)             # mutable bytes
```

## Access

```lisp
(def b (bytes 72 101 108))
(get b 0)                  # => 72
(length b)                 # => 3
```

## A slice shares its source

`slice` on immutable bytes answers bytes over the same memory. No byte is
copied. The source's pages stay alive as long as the slice does, so a slice of
a large value held for a long time keeps the whole value resident. A slice of
`@bytes` is a copy, because a mutable value can move its bytes.

```lisp
(def b (bytes 1 2 3 4 5))
(assert (= (slice b 1 3) (bytes 2 3)) "the bytes between the two indices")
(assert (= (slice b 3) (bytes 4 5)) "the end defaults to the length")
(assert (= (slice b -2) (bytes 4 5)) "a negative index counts from the end")
(assert (= (slice b 4 2) (bytes)) "a start past its end is empty")
```

The same holds for a string: `slice` on an immutable string answers a string
over the source's own bytes ([strings](strings.md)).

## Searching

`bytes/find` answers the byte index of the first occurrence of a needle, or
`nil`. The haystack and the needle are bytes or strings, mutable or not, and an
integer needle is one byte. An optional offset starts the search at that byte
index.

```lisp
(assert (= (bytes/find (bytes "a\nb\n") (bytes "\n")) 1) "the first newline")
(assert (= (bytes/find (bytes "a\nb\n") 10 2) 3) "an integer needle, from an offset")
(assert (nil? (bytes/find (bytes "ab") "\n")) "nil when the needle is absent")
(assert (= (bytes/find "héllo" "l") 3) "a byte index, not a grapheme index")
```

`string/find` counts graphemes instead, which is what text wants and what a
byte-level reader cannot afford ([strings](strings.md)).

## Hex encoding

```lisp
(seq->hex (bytes 1 2 3))           # => "010203"
(seq->hex (bytes 255))             # => "ff"
(seq->hex [1 2 3])                 # => "010203" (also works with arrays)
(seq->hex 255)                     # => "ff" (and non-negative integers)
```

`bytes->hex` is an alias for `seq->hex`.

## Conversion

`string` on immutable bytes checks that the bytes are UTF-8 and answers a
string over the same bytes, without a copy. Bytes that are not UTF-8 raise
`:encoding-error`.

```lisp
(assert (= (string (bytes 97 98 99)) "abc") "UTF-8 decode")
(let [[ok? err] (protect (string (bytes 255)))]
  (assert (not ok?) "a byte that is not UTF-8 does not decode")
  (assert (= (get err :error) :encoding-error)))
```

---

## See also

- [strings.md](strings.md) — string operations
- [types.md](types.md) — type system
