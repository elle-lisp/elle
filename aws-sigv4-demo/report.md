# AWS SigV4 Demo: Elle Capability Report

## 1. Scheme Reference Implementation Analysis

**File**: `demos/aws-sigv4/sigv4.scm` (250 lines)

The Scheme implementation contains:

- **String utilities** (lines 15-37): `char-in-string?`, `to-hex-string`, `bytevector->hex-string`
- **DateTime functions** (lines 43-79): ISO 8601 parsing, AWS date/datetime formatting
- **URL encoding** (lines 86-104): Percent-encoding with unreserved character check
- **AWS SigV4 components** (lines 110-142): Canonical headers, signed headers, query strings
- **Placeholder crypto** (lines 148-154): SHA256/HMAC-SHA256 stubs returning zero bytevectors
- **Test cases** (lines 160-250): Timestamp parsing, URI encoding, datetime formatting, hex conversion

### Scheme Primitives Used in sigv4.scm

Every Scheme function/form used, extracted from the source:

| Scheme Primitive | Line(s) in sigv4.scm | Purpose |
|---|---|---|
| `defn` | 16, 24, 34, 45, 58, 63, 70, 86, 91, 98, 111, 124, 134, 149, 152, 160, 172, 190, 202 | Function definition (prelude macro) |
| `let` | 25, 26, 49, 59, 117, 126, 163, 168, 175, 193, 197, 205 | Sequential bindings (multiple pairs) |
| `let loop ((i 0))` | 17, 26 | Named let for iteration |
| `cond` | 18-21 | Multi-way conditional |
| `else` | 21 | Default cond clause |
| `if` | 27, 101, 137 | Two-way conditional |
| `string-length` | 19, 60 | String length |
| `string-ref` | 20, 30 | Character at index (returns char) |
| `char=?` | 20 | Character equality |
| `char>=?`, `char<=?` | 92-95 | Character comparison |
| `char->integer` | 87 | Character to integer code |
| `char-in-string?` | 95 | Custom: char membership test |
| `string-append` | 28, 60, 66-78, 88, 99, 103, 114, 119-120, 125-128, 139-141 | String concatenation (variadic via apply) |
| `apply` | 28, 35-36, 99, 114, 125, 127-128, 139 | Apply function to list of args |
| `map` | 28, 36, 100, 115, 126-127, 140 | Map function over list |
| `string` | 28, 102 | Convert char to single-char string |
| `lambda` | 100, 115-116, 126-127, 140, 179 | Anonymous function |
| `string->number` | 49-54 | Parse string to number |
| `substring` | 49-54 | Extract substring by indices |
| `number->string` | 59 | Number to string |
| `make-string` | 60 | Create string of N copies of a char |
| `max` | 60 | Maximum of two numbers |
| `list` | 55, 175-178 | Create list |
| `cons` | 30 | Cons cell construction |
| `car` | 117, 126 | First of pair |
| `cdr` | 118, 129, 142 | Rest of pair |
| `null?` | 137 | Empty list check |
| `reverse` | 142 | Reverse list |
| `string-downcase` | 119, 126 | Lowercase string |
| `string-trim` | 120 | Trim whitespace |
| `string->list` | 104, 128 | String to list of characters |
| `for-each` | 179 | Iterate over list with side effects |
| `display` | 161-249 | Output without quotes |
| `newline` | 162-249 | Print newline |
| `#t`, `#f` | 19-21, 92-95 | Boolean literals |
| `#\0`, `#\a`, `#\z`, `#\A`, `#\Z`, `#\9` | 60, 92-95 | Character literals |
| `or` | 92-95 | Boolean or |
| `and` | 92-95 | Boolean and |
| `quotient` | 29 | Integer division |
| `modulo` | 30 | Integer modulus |
| `make-bytevector` | 150, 153, 205 | Create bytevector |
| `bytevector-u8-set!` | 206-209 | Set byte in bytevector |
| `bytevector->u8-list` | 37 | Bytevector to list of bytes |

## 2. Elle Primitive Mapping

### Available Primitives (with file locations)

#### String Operations

| Elle Primitive | Registered Name(s) | File | Line |
|---|---|---|---|
| `string-upcase` | `string/upcase`, `string-upcase` | `src/primitives/string.rs` | 41, 583-593 |
| `string-downcase` | `string/downcase`, `string-downcase` | `src/primitives/string.rs` | 64, 594-604 |
| `substring` | `string/slice`, `substring` | `src/primitives/string.rs` | 87, 605-615 |
| `string-index` | `string/index`, `string-index` | `src/primitives/string.rs` | 149, 616-626 |
| `char-at` | `string/char-at`, `char-at` | `src/primitives/string.rs` | 197, 627-637 |
| `string-split` | `string/split`, `string-split` | `src/primitives/string.rs` | 256, 638-648 |
| `string-replace` | `string/replace`, `string-replace` | `src/primitives/string.rs` | 301, 649-659 |
| `string-trim` | `string/trim`, `string-trim` | `src/primitives/string.rs` | 368, 660-670 |
| `string-contains?` | `string/contains?`, `string-contains?` | `src/primitives/string.rs` | 392, 671-681 |
| `string-starts-with?` | `string/starts-with?`, `string-starts-with?` | `src/primitives/string.rs` | 435, 682-692 |
| `string-ends-with?` | `string/ends-with?`, `string-ends-with?` | `src/primitives/string.rs` | 481, 693-703 |
| `string-join` | `string/join`, `string-join` | `src/primitives/string.rs` | 527, 704-714 |

**Key detail**: `char-at` returns a **single-character string**, not a character type. Elle has no character type. (`src/primitives/string.rs` line 241: `Value::string(c.to_string())`)

#### Conversion Operations

| Elle Primitive | Registered Name(s) | File | Line |
|---|---|---|---|
| `number->string` | `number->string` | `src/primitives/convert.rs` | 218, 332-343 |
| `string->integer` | `string->integer`, `string->int` | `src/primitives/convert.rs` | 251, 344-354 |
| `string->float` | `string->float` | `src/primitives/convert.rs` | 257, 355-365 |
| `string` | `string` (alias of `to-string`) | `src/primitives/convert.rs` | 91, 421-431 |
| `integer` | `integer`, `int` | `src/primitives/convert.rs` | 9, 399-409 |

**Key detail**: `string->integer` (`string->int`) is the Elle equivalent of Scheme's `string->number` for integer parsing. There is no generic `string->number`.

#### List Operations

| Elle Primitive | Registered Name(s) | File | Line |
|---|---|---|---|
| `cons` | `cons` | `src/primitives/list.rs` | 48, 768-779 |
| `first` | `first` | `src/primitives/list.rs` | 62, 780-790 |
| `rest` | `rest` | `src/primitives/list.rs` | 93, 791-801 |
| `list` | `list` | `src/primitives/list.rs` | 128, 802-812 |
| `length` | `length` | `src/primitives/list.rs` | 133, 813-823 |
| `empty?` | `empty?` | `src/primitives/list.rs` | 227, 824-834 |
| `append` | `append` | `src/primitives/list.rs` | 339, 835-845 |
| `reverse` | `reverse` | `src/primitives/list.rs` | 619, 857-867 |
| `last` | `last` | `src/primitives/list.rs` | 643, 869-879 |
| `butlast` | `butlast` | `src/primitives/list.rs` | 668, 880-890 |
| `take` | `take` | `src/primitives/list.rs` | 702, 891-901 |
| `drop` | `drop` | `src/primitives/list.rs` | 735, 902-912 |

**Key detail**: `append` takes exactly 2 arguments (`Arity::Exact(2)`, `src/primitives/list.rs` line 339). It is not variadic. For strings, `append` concatenates two strings.

#### Higher-Order Functions (defined in Elle, not native)

| Function | Defined in | Line |
|---|---|---|
| `map` | `src/primitives/higher_order_def.rs` | 8-12 |
| `filter` | `src/primitives/higher_order_def.rs` | 16-22 |
| `fold` | `src/primitives/higher_order_def.rs` | 26-30 |

```lisp
;; map definition (src/primitives/higher_order_def.rs lines 8-12):
(def map (fn (f lst)
  (if (empty? lst)
    ()
    (cons (f (first lst)) (map f (rest lst))))))
```

#### Arithmetic Operations

| Elle Primitive | Registered Name(s) | File | Line |
|---|---|---|---|
| `+`, `-`, `*`, `/` | same | `src/primitives/arithmetic.rs` | 9, 38, 64, 240 |
| `mod` | `mod` | `src/primitives/arithmetic.rs` | 92 |
| `rem` | `rem`, `%` | `src/primitives/arithmetic.rs` | 110 |
| `abs` | `abs` | `src/primitives/arithmetic.rs` | 128 |
| `min` | `min` | `src/primitives/arithmetic.rs` | 144 |
| `max` | `max` | `src/primitives/arithmetic.rs` | 169 |

**Key detail**: `mod` is Euclidean modulo (result has same sign as divisor). `rem`/`%` is truncated remainder. Scheme's `modulo` matches Elle's `mod`. Scheme's `quotient` is directly available as `(/ a b)` when both args are integers — Rust's integer division truncates toward zero (`src/arithmetic.rs` line 77).

#### Display/Output

| Elle Primitive | File | Line |
|---|---|---|
| `display` | `src/primitives/display.rs` | 8 |
| `print` (display + newline) | `src/primitives/display.rs` | 16 |
| `newline` | `src/primitives/display.rs` | 33 |

#### Bitwise Operations

| Elle Primitive | File | Line |
|---|---|---|
| `bit/and` | `src/primitives/bitwise.rs` | 9 |
| `bit/or` | `src/primitives/bitwise.rs` | 46 |
| `bit/xor` | `src/primitives/bitwise.rs` | 83 |
| `bit/not` | `src/primitives/bitwise.rs` | 120 |
| `bit/shl` / `bit/shift-left` | `src/primitives/bitwise.rs` | 144 |
| `bit/shr` / `bit/shift-right` | `src/primitives/bitwise.rs` | 200 |

#### Buffer Operations

| Elle Primitive | File | Line |
|---|---|---|
| `buffer` | `src/primitives/buffer.rs` | 11 |
| `string->buffer` | `src/primitives/buffer.rs` | 44 |
| `buffer->string` | `src/primitives/buffer.rs` | 70 |

**Key detail**: `buffer` creates a buffer from individual byte arguments: `(buffer 72 101 108)`. There is no `make-bytevector` equivalent (create buffer of N bytes with fill value). There is no `bytevector-u8-set!` or `bytevector->u8-list` equivalent. Buffer mutation uses `push`/`pop` from `src/primitives/array.rs`. Buffer indexing uses `get`/`put` from `src/primitives/table.rs`.

#### FFI Infrastructure

Full FFI system at `src/ffi/` and `src/primitives/ffi.rs`:

| Elle Primitive | Purpose | File | Line |
|---|---|---|---|
| `ffi/native` | Load shared library | `src/primitives/ffi.rs` | 89 |
| `ffi/lookup` | Symbol lookup | `src/primitives/ffi.rs` | 144 |
| `ffi/signature` | Create function signature | `src/primitives/ffi.rs` | 211 |
| `ffi/call` | Call C function | `src/primitives/ffi.rs` | 297 |
| `ffi/malloc` | Allocate C memory | `src/primitives/ffi.rs` | 531 |
| `ffi/free` | Free C memory | `src/primitives/ffi.rs` | 567 |
| `ffi/read` | Read typed value from memory | `src/primitives/ffi.rs` | 610 |
| `ffi/write` | Write typed value to memory | `src/primitives/ffi.rs` | 681 |
| `ffi/string` | Read C string from pointer | `src/primitives/ffi.rs` | 891 |
| `ffi/callback` | Create C callback from closure | `src/primitives/ffi.rs` | 948 |
| `ffi/defbind` | Convenient FFI binding macro | `prelude.lisp` | 117-132 |

FFI example at `examples/ffi.lisp` demonstrates: library loading, function binding, memory management, struct marshalling, variadic calls, and callbacks.

### Special Forms

| Elle Form | Purpose | Analyzer Location |
|---|---|---|
| `if` | Conditional | `src/hir/analyze/forms.rs` line 204 |
| `cond` | Multi-way conditional | `src/hir/analyze/forms.rs` line 544 |
| `let` | Parallel bindings | `src/hir/analyze/forms.rs` line 153 |
| `let*` | Sequential bindings (prelude macro) | `prelude.lisp` line 14 |
| `letrec` | Recursive bindings | `src/hir/analyze/forms.rs` line 154 |
| `while` | While loop | `src/hir/analyze/forms.rs` line 395 |
| `each` | For-each loop | `src/hir/analyze/forms.rs` line 437 |
| `def` | Immutable binding | `src/hir/analyze/forms.rs` line 160 |
| `var` | Mutable binding | `src/hir/analyze/forms.rs` line 159 |
| `set` | Mutation | `src/hir/analyze/forms.rs` line 161 |
| `fn` | Lambda | `src/hir/analyze/forms.rs` line 155 |
| `defn` | Function definition (prelude macro) | `prelude.lisp` line 9 |
| `match` | Pattern matching | `src/hir/analyze/forms.rs` line 174 |
| `and`, `or` | Short-circuit boolean | `src/hir/analyze/forms.rs` lines 164-165 |
| `begin` | Sequencing (no scope) | `src/hir/analyze/forms.rs` line 156 |
| `block` | Scoped sequencing with break | `src/hir/analyze/forms.rs` line 157 |
| `quote` | Quote | `src/hir/analyze/forms.rs` line 166 |
| `yield` | Yield from coroutine | `src/hir/analyze/forms.rs` line 173 |
| `defmacro` | Macro definition | (handled by expander) |

**`cond` details** (`src/hir/analyze/forms.rs` lines 544-595):
- Clauses are parenthesized: `(cond ((test1) body1) ((test2) body2) (else default))`
- `else` is recognized as a keyword for the default branch (line 574)
- `true` also works as a default clause (it evaluates to true)

**`each` details** (`src/hir/analyze/forms.rs` lines 437-481):
- Syntax: `(each var iter body)` or `(each var in iter body)`
- Binds `var` to each element of `iter` and evaluates `body`
- This is Elle's equivalent of Scheme's `for-each`

## 3. Primitive Mapping: Scheme to Elle

| Scheme Primitive | Elle Equivalent | Status | Notes |
|---|---|---|---|
| `(defn name (params) body)` | `(defn name (params) body)` | AVAILABLE | Identical syntax. Prelude macro at `prelude.lisp` line 9 |
| `(let ((a 1) (b 2)) body)` | `(let ((a 1) (b 2)) body)` | AVAILABLE | Same syntax |
| `(let loop ((i 0)) body)` | N/A | **MISSING** | Named let for iteration. Use `letrec` + named `fn`, or `while` with mutable vars |
| `(cond (test body) (else default))` | `(cond ((test) body) (else default))` | AVAILABLE | Same syntax. `src/hir/analyze/forms.rs` line 544 |
| `(if test then else)` | `(if test then else)` | AVAILABLE | Same |
| `(lambda (args) body)` | `(fn (args) body)` | AVAILABLE | Different keyword |
| `(string-length str)` | `(length str)` | AVAILABLE | `length` is polymorphic. `src/primitives/list.rs` line 166 |
| `(string-ref str i)` | `(char-at str i)` | AVAILABLE (different return type) | Returns single-char **string**, not char. `src/primitives/string.rs` line 240 |
| `(char=? c1 c2)` | `(= c1 c2)` | AVAILABLE | `=` uses `PartialEq`, works on strings. `src/primitives/comparison.rs` line 21 |
| `(char>=? c d)` | N/A | **MISSING** | `>=` only works on numbers, not strings. `src/primitives/comparison.rs` line 131 |
| `(char<=? c d)` | N/A | **MISSING** | `<=` only works on numbers, not strings. `src/primitives/comparison.rs` line 101 |
| `(char->integer c)` | N/A | **MISSING** | No char type, no char->integer. `char-at` returns strings |
| `#\0`, `#\a`, etc. | N/A | **MISSING** | No character literals in the reader. Token types at `src/reader/token.rs` lines 73-92 have no Char variant |
| `(string c)` | N/A | **MISSING (for char->string)** | Scheme's `(string char)` creates a 1-char string. Elle's `(string x)` converts any value to string repr |
| `(string->list str)` | N/A | **MISSING** | No string-to-list-of-chars. Would need to build with `char-at` in a loop |
| `(string->number str)` | `(string->int str)` / `(string->float str)` | AVAILABLE (split into two) | `src/primitives/convert.rs` lines 251, 257. Must choose int or float |
| `(substring str start end)` | `(substring str start end)` | AVAILABLE | Same syntax. `src/primitives/string.rs` line 87 |
| `(string-append s1 s2 ...)` | `(append s1 s2)` | PARTIAL | `append` is binary only (2 args). `src/primitives/list.rs` line 339 (arity check line 341) |
| `(apply string-append list)` | N/A | **MISSING** | No `apply` primitive. No variadic `string-append` |
| `(string-downcase str)` | `(string-downcase str)` | AVAILABLE | `src/primitives/string.rs` line 64 |
| `(string-trim str)` | `(string-trim str)` | AVAILABLE | `src/primitives/string.rs` line 368 |
| `(number->string n)` | `(number->string n)` | AVAILABLE | `src/primitives/convert.rs` line 218 |
| `(make-string n char)` | N/A | **MISSING** | No `make-string`. No way to create a string of N copies of a char |
| `(max a b)` | `(max a b)` | AVAILABLE | `src/primitives/arithmetic.rs` line 169 |
| `(quotient a b)` | `(/ a b)` | AVAILABLE | Integer `/` truncates toward zero, same as `quotient`. `src/arithmetic.rs` line 77 |
| `(modulo a b)` | `(mod a b)` | AVAILABLE | Euclidean modulo. `src/primitives/arithmetic.rs` line 92 |
| `(map f lst)` | `(map f lst)` | AVAILABLE | Defined in Elle at `src/primitives/higher_order_def.rs` line 8 |
| `(for-each f lst)` | `(each var lst body)` | AVAILABLE (different syntax) | Special form, not a function. `src/hir/analyze/forms.rs` line 163 |
| `(display val)` | `(display val)` | AVAILABLE | `src/primitives/display.rs` line 8 |
| `(newline)` | `(newline)` | AVAILABLE | `src/primitives/display.rs` line 33 |
| `(cons a b)` | `(cons a b)` | AVAILABLE | `src/primitives/list.rs` line 48 |
| `(car x)` | `(first x)` | AVAILABLE | `src/primitives/list.rs` line 62 |
| `(cdr x)` | `(rest x)` | AVAILABLE | `src/primitives/list.rs` line 93 |
| `(null? x)` | `(empty? x)` or `(nil? x)` | AVAILABLE (see notes) | `nil?` checks for `nil`. `empty?` checks for empty collections. Per AGENTS.md: lists are EMPTY_LIST-terminated, use `empty?` for end-of-list |
| `(reverse lst)` | `(reverse lst)` | AVAILABLE | `src/primitives/list.rs` line 619 |
| `(list a b c)` | `(list a b c)` | AVAILABLE | `src/primitives/list.rs` line 128 |
| `(apply f args)` | N/A | **MISSING** | No `apply` primitive. Not found in any primitive registration |
| `#t` / `#f` | `true` / `false` | AVAILABLE | `src/reader/token.rs` line 91: `Token::Bool(bool)` |
| `(or a b)` | `(or a b)` | AVAILABLE | Special form. `src/hir/analyze/forms.rs` line 165 |
| `(and a b)` | `(and a b)` | AVAILABLE | Special form. `src/hir/analyze/forms.rs` line 164 |
| `(make-bytevector n fill)` | N/A | **MISSING** | `buffer` takes individual bytes, no size+fill constructor |
| `(bytevector-u8-set! bv i byte)` | `(put buf i byte)` | NEEDS VERIFICATION | `put` works on arrays/tables; buffer support needs testing |
| `(bytevector->u8-list bv)` | N/A | **MISSING** | No buffer-to-list-of-bytes conversion |

## 4. Existing Demo Patterns and Elle Idioms

### How existing Elle demos handle patterns from sigv4.scm

#### Function Definition

**nqueens.lisp** (line 6-14) uses `var` + `fn`:
```lisp
(var check-safe-helper
  (fn (col remaining row-offset)
    (if (empty? remaining)
      true
      ...)))
```

**fib.lisp** (line 4-6) uses `defn`:
```lisp
(defn fib (n)
  (if (< n 2) n
    (+ (fib (- n 1)) (fib (- n 2)))))
```

Both forms work. `defn` is a prelude macro expanding to `(def name (fn ...))`. The Scheme `defn` in sigv4.scm is identical to Elle's `defn`.

#### Iteration (replacing named let)

The Scheme `(let loop ((i 0)) ...)` pattern has no direct equivalent. Elle idioms for iteration:

1. **Recursive function via `letrec`** (used in `prelude.lisp` line 122-126):
```lisp
(letrec ((gen-params (fn (i acc)
                       (if (= i arg-count)
                         (reverse acc)
                         (gen-params (+ i 1) (cons (gensym) acc))))))
  (gen-params 0 '()))
```

2. **`while` with mutable variable** (examples in `examples/scope-and-binding.lisp`):
```lisp
(var i 0)
(while (< i n)
  (begin ... (set i (+ i 1))))
```

3. **`each` for iteration over collections** (`src/hir/analyze/forms.rs` line 437):
```lisp
(each x (list 1 2 3)
  (display x))
```

4. **Tail-recursive function** (used in `demos/nqueens/nqueens.lisp`):
```lisp
(var helper (fn (i result)
  (if (= i 0) result
    (helper (- i 1) (cons i result)))))
```

#### String Concatenation (replacing variadic string-append + apply)

The Scheme pattern `(apply string-append (map f lst))` is heavily used in sigv4.scm. Elle alternatives:

1. **`string-join` with empty separator** (`src/primitives/string.rs` line 527):
```lisp
(string-join (map f lst) "")
```
This is the direct replacement for `(apply string-append (map f lst))`.

2. **`fold` with `append`**:
```lisp
(fold (fn (acc s) (append acc s)) "" (map f lst))
```

3. **Binary `append` for two strings** (`src/primitives/list.rs` line 414-420):
```lisp
(append "hello" " world")
```

#### Character Operations (replacing char literals and char functions)

Elle has no character type. `char-at` returns a single-character string.

Scheme pattern:
```scheme
(char>=? c #\a)  ; character comparison
(char->integer c) ; character to codepoint
```

Elle workaround pattern:
```lisp
(def ch (char-at str i))    ; ch is a "a" style string
(>= ch "a")                 ; string comparison (needs verification)
```

**Critical gap**: `char->integer` has no equivalent. To get the codepoint of a character, there is no built-in. Would need to build a lookup table or add a primitive.

#### Alist vs Table/Struct

The Scheme sigv4.scm uses alists (association lists) for headers:
```scheme
(let ((name (car header))
      (value (cdr header)))
  ...)
```

Elle could use:
1. **Same alist pattern** with `(first header)` / `(rest header)` on cons pairs
2. **Structs** (immutable): `{:name "host" :value "example.com"}`
3. **Tables** (mutable): `@{:name "host" :value "example.com"}`

The Scheme sigv4.scm's alist approach is compatible with Elle's cons cells.

## 5. Gap Analysis Summary

### Missing Primitives (Required for Direct Port)

| Missing Item | Used In (sigv4.scm lines) | Severity | Workaround |
|---|---|---|---|
| `apply` | 28, 35-36, 99, 114, 125, 127-128, 139 | HIGH | Replace `(apply string-append lst)` with `(string-join lst "")` |
| Variadic `string-append` | many | HIGH | `(string-join list "")` or chained `(append a b)` |
| `char->integer` | 87 | HIGH | No workaround without new primitive or lookup table |
| `char>=?` / `char<=?` (ordering on chars) | 92-95 | HIGH | `>=`/`<=` only work on numbers. Need `char->integer` or `string-contains?` workaround |
| Character literals (`#\a`, etc.) | 60, 92-95 | HIGH | Use string literals `"a"`, `"z"`, `"0"`, `"9"`, etc. |
| `string->list` | 104, 128 | MEDIUM | Build with loop: `(letrec ((f (fn (i acc) ...))) ...)` |
| `make-string` (n, fill-char) | 60 | MEDIUM | Build with loop or fold |
| Named `let` loops | 17, 26 | MEDIUM | Use `letrec` with named function |
| `string-ref` returning char | 20, 30 | LOW | `char-at` returns string instead; works for most purposes |
| `quotient` | 29 | NONE | `(/ a b)` — integer `/` truncates toward zero, same as `quotient` (`src/arithmetic.rs` line 77) |
| `bytevector->u8-list` | 37 | LOW | Only needed for hex conversion of crypto output |
| `make-bytevector` (with fill) | 150, 153, 205 | LOW | Crypto stubs can use `(buffer 0 0 0 ...)` or FFI malloc |
| `bytevector-u8-set!` | 206-209 | LOW | Only in test case; can use FFI write or `put` on buffer |
| `for-each` (as function) | 179 | LOW | `each` special form works for all use cases |
| `null?` | 137 | LOW | `empty?` for list termination check |

### Missing Primitives (Required for Real Crypto via FFI)

The FFI system (`src/ffi/`, `src/primitives/ffi.rs`) is fully capable of calling OpenSSL or other crypto libraries. The `examples/ffi.lisp` file demonstrates all necessary patterns. To implement SHA256/HMAC-SHA256:

1. Load libcrypto: `(def crypto (ffi/native "libcrypto.so"))`
2. Bind functions: `(ffi/defbind SHA256 crypto "SHA256" :ptr @[:ptr :size :ptr])`
3. Allocate output buffers: `(def out (ffi/malloc 32))`
4. Call functions and read results

The FFI supports: pointer types (`:ptr`), sized integers (`:i32`, `:u8`, etc.), strings (`:string`), structs, arrays, callbacks, variadic functions, and memory management.

## 6. Existing Tests for Relevant Operations

### String Operations Tests

- `examples/string-operations.lisp` (253 lines): Tests `string-split`, `string-replace`, `string-trim`, `string-contains?`, `string-starts-with?`, `string-ends-with?`, `string-join`, `number->string`, `string-upcase`, `string-downcase`, `substring`, `char-at`, `string-index`
- `tests/integration/buffer.rs` line 56: `test_string_to_buffer`

### Higher-Order Function Tests

- `examples/higher-order-functions.lisp` (413 lines): Tests `map`, `filter`, `fold`, composition, currying

### FFI Tests

- `examples/ffi.lisp` (55 lines): Exercises entire FFI pipeline
- `tests/integration/ffi.rs` line 343: `test_ffi_string_from_buffer`
- `src/primitives/ffi.rs` lines 1253-1578: Extensive unit tests for FFI primitives

### Control Flow Tests

- `examples/control-flow.lisp` (277 lines): Tests `cond` with `true` default, `match`, `while`
- `src/pipeline.rs` lines 768-798: `cond` evaluation tests

## 7. Critical Findings for the Port

### `char-at` Returns Strings, Not Characters

At `src/primitives/string.rs` line 240-241:
```rust
match s.chars().nth(index) {
    Some(c) => (SIG_OK, Value::string(c.to_string())),
```

This means all character operations must work on single-character strings.

### Comparison Operators (`<`, `>`, `<=`, `>=`) Do NOT Support Strings

At `src/primitives/comparison.rs` lines 30-57, 60-87, 90-117, 120-147: all ordering comparison operators only accept numbers (int or float). They return a type-error for strings:
```rust
return (SIG_ERROR, error_val("type-error", format!("<: expected number, got {}", args[0].type_name())));
```

**Consequence**: Scheme's `(char>=? c #\a)` cannot be replicated with `(>= (char-at str i) "a")` — this will error. Character range checks like `uri-unreserved?` (sigv4.scm lines 91-95) require a different approach: either a `char->integer` primitive for numeric comparison, or use `string-contains?` / `string-index` for membership testing.

The `=` operator (line 9-27) uses Rust's `PartialEq` (`args[0] == args[1]`), which does work for string equality comparison.

### No `apply` Function

`apply` is not registered in any primitive table. It does not appear in `src/primitives/registration.rs` (which lists all primitive tables, lines 14-45). The LSP rename module lists it as a reserved word (`src/lsp/rename.rs` line 33) but it is not implemented.

### `append` is Binary Only

`src/primitives/list.rs` line 341:
```rust
if args.len() != 2 {
    return (SIG_ERROR, error_val("arity-error", ...));
}
```

For string concatenation, `append` works on exactly 2 strings. Multiple string concatenation requires chaining or using `string-join`.

### `string-join` is the Key Workaround

`string-join` (`src/primitives/string.rs` line 527) takes a list of strings and a separator. Using `""` as separator gives the equivalent of `(apply string-append list)`:

```lisp
;; Scheme:
(apply string-append (map f lst))
;; Elle:
(string-join (map f lst) "")
```

### `each` is a Special Form, Not a Function

`each` is analyzed as a special form at `src/hir/analyze/forms.rs` line 163. Syntax:
```lisp
(each x (list 1 2 3)
  (display x))
```
It cannot be passed as a value or stored in a variable. It replaces `for-each` but with different syntax.

### Boolean Literals

Elle uses `true`/`false` (not `#t`/`#f`). Token type at `src/reader/token.rs` line 91.

### No Character Type in the Reader

The token types at `src/reader/token.rs` lines 73-92 include: `LeftParen`, `RightParen`, `LeftBracket`, `RightBracket`, `LeftBrace`, `RightBrace`, `Quote`, `Quasiquote`, `Unquote`, `UnquoteSplicing`, `ListSugar`, `Symbol`, `Keyword`, `Integer`, `Float`, `String`, `Bool`, `Nil`. There is no `Char` variant.

### `cond` Uses `else` Keyword

Elle's `cond` recognizes `else` as a default clause (`src/hir/analyze/forms.rs` line 574). It also accepts `true` as a condition that always matches (as in `examples/control-flow.lisp` line 34).

### `string-index` Takes a String, Not a Char

`string-index` (`src/primitives/string.rs` line 149) requires the second argument to be a **single-character string** (validated at line 168: `if chars.len() != 1`). This matches the pattern of Elle not having a character type.

### `string/append` Listed in Symbols/Lint But Not Registered

`string/append` appears in `src/symbols/mod.rs` line 194 and `src/lint/rules.rs` line 138 but there is no corresponding primitive implementation or registration. This may be a planned but unimplemented feature, or it may be an alias that `append` handles when called on strings.

### `/` on Two Integers Performs Truncated Integer Division

At `src/arithmetic.rs` lines 71-78:
```rust
(Some(x), Some(y)) => {
    if y == 0 {
        return Err("Division by zero".to_string());
    }
    Ok(Value::int(x / y))
}
```

When both arguments are integers, `/` uses Rust's integer division (truncated toward zero). So `(/ 17 16)` returns `1`, not `1.0625`. This means `(/ a b)` with integer args is equivalent to Scheme's `quotient` for positive numbers. For negative numbers, Rust's `/` truncates toward zero (same as Scheme's `quotient`). No need for `(floor (/ a b))` workaround — plain `/` works.
