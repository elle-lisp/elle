# Bytes

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

## Hex encoding

```lisp
(seq->hex (bytes 1 2 3))           # => "010203"
(seq->hex (bytes 255))             # => "ff"
(seq->hex [1 2 3])                 # => "010203" (also works with arrays)
(seq->hex 255)                     # => "ff" (and non-negative integers)
```

`bytes->hex` is an alias for `seq->hex`.

## Conversion

```lisp
(string (bytes 97 98 99))  # => "abc" (UTF-8 decode)
```

---

## See also

- [strings.md](strings.md) — string operations
- [types.md](types.md) — type system
