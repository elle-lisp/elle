# What's New in Elle

A summary of recent additions and improvements to the Elle language.

## Recent Additions

### Destructuring

Elle now supports compiler-level destructuring in `def`, `var`, `let`, `let*`,
and function parameters. Inspired by Janet's destructuring semantics.

**List destructuring:**

```lisp
(def (a b c) (list 1 2 3))
a ⟹ 1, b ⟹ 2, c ⟹ 3
```

**Array destructuring:**

```lisp
(def [x y] [10 20])
x ⟹ 10, y ⟹ 20
```

**Wildcard `_`** — skip elements you don't need:

```lisp
(def (_ b _) (list 1 2 3))
b ⟹ 2
```

**Rest patterns `& name`** — collect remaining elements:

```lisp
(def (head & tail) (list 1 2 3 4))
head ⟹ 1, tail ⟹ (2 3 4)

(def [first & others] [10 20 30])
first ⟹ 10, others ⟹ [20 30]
```

**Nested patterns:**

```lisp
(def ((a b) c) (list (list 1 2) 3))
a ⟹ 1, b ⟹ 2, c ⟹ 3
```

**In function parameters:**

```lisp
(defn add-pair ((a b)) (+ a b))
(add-pair (list 3 4)) ⟹ 7
```

**Silent nil semantics:** Missing values become `nil`, wrong types produce
`nil` for all bindings. No runtime errors from destructuring.

See `examples/destructuring.lisp` for comprehensive examples.

### defn

`defn` is a shorthand for defining named functions:

```lisp
(defn add (x y) (+ x y))
; equivalent to: (def add (fn (x y) (+ x y)))
```

The old `(def (f x) body)` shorthand has been removed in favor of `defn`.

---

## nil vs Empty List: The Full Picture

Elle distinguishes between `nil` (absence of value) and `()` (empty list). They are **different values** with different NaN-boxed tags:
- `nil` = `0x7FFC_0000_0000_0000`
- `()` = `0x7FFC_0000_0000_0003`

### Core Distinction

- **`nil`** represents the absence of a value. It is falsy and used for:
  - Functions that return "nothing" (like `display`)
  - Default/missing values
  - Logical false in conditions

- **`()`** represents an empty list. It is:
  - A valid list (just with no elements)
  - The terminator for proper lists
  - **Truthy** (because it IS a value, not the absence of one)

### Truthiness Table

| Value | Truthy? | Notes |
|-------|---------|-------|
| `#f` | ✗ No | Boolean false |
| `nil` | ✗ No | Absence of value |
| `()` | ✓ Yes | Empty list (distinct from nil) |
| `0` | ✓ Yes | Zero is truthy |
| `""` | ✓ Yes | Empty string is truthy |
| `[]` | ✓ Yes | Empty array is truthy |
| All other values | ✓ Yes | Default |

**Only `#f` and `nil` are falsy.** Everything else, including the empty list, is truthy.

### Predicate Behavior

This is the critical distinction. Each predicate behaves differently:

| Expression | Result | Notes |
|------------|--------|-------|
| `(nil? nil)` | `#t` | Only nil is nil |
| `(nil? ())` | `#f` | Empty list is NOT nil |
| `(empty? nil)` | error | Nil is not a container |
| `(empty? ())` | `#t` | Empty list is empty |
| `(list? ())` | `#t` | Empty list is a list |
| `(list? nil)` | `#f` | Nil is not a list |
| `(pair? ())` | `#f` | Empty list is not a pair |
| `(pair? nil)` | `#f` | Nil is not a pair |

### Equality

`nil` and `()` are **not equal**:

```lisp
(= nil ())   ; ⟹ #f
(eq? nil ()) ; ⟹ #f
```

### List Construction

Lists terminate with `EMPTY_LIST`, not `NIL`:

```lisp
(list 1 2 3)
; = cons(1, cons(2, cons(3, EMPTY_LIST)))
; NOT cons(1, cons(2, cons(3, NIL)))

(first (list 1 2 3))  ; ⟹ 1
(rest (list 1 2 3))   ; ⟹ (2 3)
(rest (rest (rest (list 1 2 3))))  ; ⟹ ()
```

### Migration Guidance

When walking a list, use `(empty? lst)` to check for termination, **not** `(nil? lst)`. The empty list is the proper terminator.

**WRONG** — will infinite-loop or error when lst reaches `()`:

```lisp
(def (my-map f lst)
  (if (nil? lst) ()
    (cons (f (first lst)) (my-map f (rest lst)))))
```

**RIGHT** — correctly terminates on empty list:

```lisp
(def (my-map f lst)
  (if (empty? lst) ()
    (cons (f (first lst)) (my-map f (rest lst)))))
```

The distinction matters because `(nil? ())` returns `#f`, so the wrong version will try to call `(first ())` and `(rest ())` on an empty list, causing an error or infinite loop.

### Examples

```lisp
; Empty list is truthy
(if () "truthy" "falsy")  ⟹ "truthy"

; Only #f is falsy among booleans
(if #f "truthy" "falsy")  ⟹ "falsy"

; nil is FALSY (represents absence/undefined)
(if nil "truthy" "falsy") ⟹ "falsy"

; Use nil? to check for nil specifically
(if (nil? x) "is nil" "not nil")

; Use empty? to check for empty collections
(if (empty? x) "is empty" "not empty")

; Proper list termination
(rest (list 1))  ; ⟹ ()
(nil? (rest (list 1)))  ; ⟹ #f (it's not nil!)
(empty? (rest (list 1)))  ; ⟹ #t (it's empty)
```

See `docs/semantics.md` for the authoritative specification.

## Recent Additions

### `fn` Keyword

The `fn` keyword is now the preferred way to create anonymous functions:

```lisp
; New style (preferred)
(def add (fn (a b) (+ a b)))
(map (fn (x) (* x 2)) '(1 2 3))

; Old style (still works as alias)
(var add (lambda (a b) (+ a b)))
```

The `lambda` keyword remains available as an alias for `fn`.

### Exception Handling (try-catch-finally)

Elle now provides comprehensive exception handling with the `try-catch-finally` construct:

```lisp
(try
  (risky-operation)
  (catch (e)
    (handle-error e))
  (finally
    (cleanup-resources)))
```

**Features:**
- `try` wraps code that might throw exceptions
- `catch` handles exceptions with access to the error value
- `finally` ensures cleanup code always runs
- Returns the value from `try` block or `catch` block
- `finally` block's value is discarded

**Examples:**

```lisp
(try
  (/ 10 0)
  (catch (e)
    (display "Division by zero: ")
    (display e)
    0))
⟹ 0

(try
  (display "Opening file")
  (newline)
  (file-contents)
  (catch (e)
    (display "Failed to read")
    "")
  (finally
    (display "Closed file")
    (newline)))
```

### Condition System

> **Deprecated.** See `docs/fibers.md` for the replacement design.

Beyond simple exceptions, Elle provides a sophisticated condition system for handling expected error scenarios:

```lisp
(var-condition :validation-error
  (message "Validation failed")
  (field "unknown"))

(var-handler :validation-error
  (fn (c)
    (display "Error in ")
    (display (condition-get c 'field))
    (newline)))

(signal :validation-error
  :message "Email is invalid"
  :field "email")
```

**Features:**
- Define custom condition types with fields
- Register multiple handlers per condition
- Signal conditions to trigger handlers
- Catch specific conditions with `catch-condition`
- Generic condition catching with `condition-catch`

This is useful for:
- Input validation with descriptive error reporting
- Network errors with retry logic
- Permission/authentication errors with user prompts
- Logging and monitoring

### Exception Values

Create and throw exception objects:

```lisp
(var e (exception "Error message" data))
(exception-message e)  ⟹ "Error message"
(exception-data e)     ⟹ data

(throw e)
(throw (exception "Quick error" nil))
```

---

## Primitive Naming Standardization

Recent updates have standardized Elle's primitive names to match Clojure conventions and common Lisp idioms.

### Updated Primitive Names

| Old Name | New Name | Reason |
|----------|----------|--------|
| `read-file` | `slurp` | Idiomatic file I/O naming |
| `write-file` | `spit` | Companion to slurp |
| `has?` | `has-key?` | Clarifies table key checking |
| `bool?` | `boolean?` | Standard Lisp predicate naming |
| `remainder` | `rem` | Aligns with Scheme/CL conventions |

This brings Elle into better alignment with:
- **Clojure** (~20K-50K developers) - uses slurp/spit, has-key?, etc.
- **Janet** (~500-2K developers) - modern Lisp conventions
- **Scheme/Common Lisp** - established naming standards

### Migration Guide

If you have existing Elle code:

```lisp
; Old code
(read-file "data.txt")
(write-file "output.txt" data)
(has? table "key")
(remainder 10 3)
(def (my-bool? x) (boolean? x))

; Updated code
(slurp "data.txt")
(spit "output.txt" data)
(has-key? table "key")
(rem 10 3)
(def (my-boolean? x) (boolean? x))
```

---

## File I/O Functions

### Renamed: slurp and spit

File I/O was renamed for idiom consistency with Clojure/Janet:

```lisp
; Read entire file (old: read-file)
(var content (slurp "path/to/file.txt"))

; Write to file - overwrites (old: write-file)
(spit "output.txt" content)

; Append to file (unchanged)
(append-file "log.txt" "New log entry\n")
```

### File Information

```lisp
(file-exists? "path.txt")        ⟹ #t
(file? "path.txt")               ⟹ #t
(directory? "path/")             ⟹ #t
(file-size "path.txt")           ⟹ 1024
```

### Directory Operations

```lisp
(create-directory "new-dir")
(create-directory-all "a/b/c/d")
(list-directory ".")
(delete-directory "empty-dir")
```

### Path Manipulation

```lisp
(file-name "/path/to/file.txt")      ⟹ "file.txt"
(file-extension "/path/to/file.txt") ⟹ ".txt"
(parent-directory "/path/to/file.txt") ⟹ "/path/to"
(absolute-path "relative.txt")       ⟹ "/full/path/to/relative.txt"
(join-path "dir1" "dir2" "file.txt") ⟹ "dir1/dir2/file.txt"
(current-directory)                  ⟹ "/home/user"
(change-directory "/tmp")            ; Switch working directory
```

### File Manipulation

```lisp
(copy-file "source.txt" "dest.txt")
(rename-file "old.txt" "new.txt")
(delete-file "to-delete.txt")
(read-lines "file.txt")              ⟹ ("line1" "line2" "line3")
```

---

## Higher-Order Functions

Elle supports functional programming with powerful higher-order functions:

### map

Apply a function to each element:

```lisp
(map (fn (x) (* x 2)) (list 1 2 3))     ⟹ (2 4 6)
(map string-upcase (list "a" "b" "c"))      ⟹ ("A" "B" "C")
(map abs (list -1 -2 3 -4))                 ⟹ (1 2 3 4)
```

### filter

Select elements matching a predicate:

```lisp
(filter (fn (x) (> x 2)) (list 1 2 3 4))  ⟹ (3 4)
(filter even? (list 1 2 3 4 5 6))             ⟹ (2 4 6)
(filter (fn (s) (> (length s) 3)) 
  (list "hi" "hello" "world" "a"))            ⟹ ("hello" "world")
```

### fold (reduce)

Accumulate a result:

```lisp
(fold (fn (acc x) (+ acc x)) 0 (list 1 2 3 4))  ⟹ 10
(fold (fn (acc x) (cons x acc)) nil (list 1 2 3)) ⟹ (3 2 1)
(fold (fn (acc s) (string-append acc " " s)) 
  "" (list "hello" "world"))                        ⟹ " hello world"
```

### apply

Call a function with arguments from a list:

```lisp
(apply + (list 1 2 3))                 ⟹ 6
(apply (fn (a b c) (+ a b c)) 
  (list 10 20 30))                     ⟹ 60
```

---

## String Operations

### Case Conversion

```lisp
(string-upcase "hello")    ⟹ "HELLO"
(string-downcase "HELLO")  ⟹ "hello"
```

### String Analysis

```lisp
(length "hello")           ⟹ 5
(string-contains? "hello" "ell")  ⟹ #t
(string-starts-with? "hello" "he")⟹ #t
(string-ends-with? "hello" "lo")  ⟹ #t
(string-index "hello" "ll")       ⟹ 2
(char-at "hello" 0)               ⟹ "h"
```

### String Manipulation

```lisp
(string-append "hello" " " "world") ⟹ "hello world"
(substring "hello" 1 4)             ⟹ "ell"
(string-split "a,b,c" ",")          ⟹ ("a" "b" "c")
(string-replace "hello" "l" "L")    ⟹ "heLLo"
(string-trim "  hello  ")           ⟹ "hello"
(string-join (list "a" "b" "c") ",") ⟹ "a,b,c"
```

### Type Conversions

```lisp
(int "42")          ⟹ 42
(float "3.14")      ⟹ 3.14
(string 42)         ⟹ "42"
(number->string 42) ⟹ "42"
```

---

## Collection Operations

### Tables (Mutable Hash Maps)

```lisp
(var t (table "x" 10 "y" 20))
(get t "x")          ⟹ 10
(put t "z" 30)       ; Modifies t
(del t "x")          ; Removes "x" from t
(has-key? t "x")     ⟹ #f (now deleted)
(keys t)             ⟹ ("y" "z")
(values t)           ⟹ (20 30)
(length t)     ⟹ 2
```

### Structs (Immutable Hash Maps)

```lisp
(var s (struct "a" 1 "b" 2))
(struct-get s "a")          ⟹ 1
(var s2 (struct-put s "c" 3))  ; Returns new struct
(struct-del s2 "b")         ; Returns new struct without "b"
(struct-has? s2 "c")        ⟹ #t
(struct-keys s)             ⟹ ("a" "b")
(struct-values s)           ⟹ (1 2)
(length s)           ⟹ 2
```

### Arrays

```lisp
(var v [1 2 3])
(length v)           ⟹ 3
(array-ref v 1)      ⟹ 2
(array-set! v 0 99)  ⟹ [99 2 3]
```

### Lists

```lisp
(first (list 1 2 3))  ⟹ 1
(rest (list 1 2 3))   ⟹ (2 3)
(append (list 1 2) (list 3 4)) ⟹ (1 2 3 4)
(reverse (list 1 2 3)) ⟹ (3 2 1)
(take 2 (list 1 2 3 4)) ⟹ (1 2)
(drop 2 (list 1 2 3 4)) ⟹ (3 4)
(length (list 1 2 3)) ⟹ 3
(nth 1 (list 'a 'b 'c')) ⟹ b
(last (list 1 2 3)) ⟹ 3
```

---

## JSON Operations

```lisp
; Parse JSON string to table
(var data (json-parse "{\"x\": 42, \"y\": \"hello\"}"))
(get data "x") ⟹ 42

; Serialize table to JSON
(json-serialize (table "x" 42 "y" "hello"))
⟹ "{\"x\":42,\"y\":\"hello\"}"

; Pretty-print JSON
(json-serialize-pretty (table "x" 42 "y" "hello"))
⟹ "{\n  \"x\": 42,\n  \"y\": \"hello\"\n}"
```

---

## Concurrency

```lisp
; Create and run a thread
(spawn (fn ()
  (display "Running in thread")
  (newline)))

; Create and wait for result
(var t (spawn (fn () (+ 2 2))))
(join t)  ⟹ 4

; Sleep current thread
(time/sleep 1)      ; Sleep 1 second
(time/sleep 0.5)    ; Sleep 500ms

; Get current thread ID
(current-thread-id) ⟹ "ThreadId(1)"
```

---

## Type System Improvements

### Type Checking Predicates

All type predicates end with `?`:

```lisp
(nil? nil)          ⟹ #t
(boolean? #t)       ⟹ #t
(number? 42)        ⟹ #t
(symbol? 'x)        ⟹ #t
(string? "hello")   ⟹ #t
(pair? (list 1 2))  ⟹ #t
```

### Type Name

Get the type name as a keyword:

```lisp
(type-of 42)     ⟹ :integer
(type-of 3.14)   ⟹ :float
(type-of "hello")⟹ :string
(type-of #t)     ⟹ :boolean
(type-of 'x)     ⟹ :symbol
(type-of (list)) ⟹ :list
```

---

## Debugging

```lisp
(debug-print 42)              ; Prints: [DEBUG] 42
(trace "label" (+ 1 2))       ; Prints: [TRACE] label: 3, returns 3
(memory-usage)                 ; Returns (rss-bytes virtual-bytes)
```

---

## Module System

```lisp
; Load an external module
(import-file "lib/helpers.elle")

; Add custom search paths
(add-module-path "/opt/elle-libs")

; Get package info
(package-version) ⟹ "0.3.0"
(package-info)    ⟹ ("Elle" "0.3.0" "description...")
```

---

## Migration Guide

### From Older Elle Versions

If upgrading from pre-exception-handling versions:

**Old Pattern (using define with result):**
```lisp
(var result (if (can-do?)
  (do-it)
  default-value))
```

**New Pattern (using try-catch):**
```lisp
(var result (try
  (do-it)
  (catch (e)
    default-value)))
```

**Old Pattern (custom error handling):**
```lisp
(if (valid-input? x)
  (process x)
  (display "Error!"))
```

**New Pattern (condition system):**
```lisp
(catch-condition :validation-error
  (validate x)
  (fn (c)
    (display "Error: ")
    (display (condition-get c 'message))))
```

---

## Summary of Key Features

| Feature | Version | Status |
|---------|---------|--------|
| Destructuring | Recent | Stable |
| defn | Recent | Stable |
| fn keyword | Recent | Stable |
| try-catch-finally | Recent | Stable |
| Condition system | Recent | Stable |
| Exception objects | Recent | Stable |
| slurp/spit naming | Recent | Stable |
| boolean? predicate | Recent | Stable |
| has-key? predicate | Recent | Stable |
| rem instead of remainder | Recent | Stable |
| Comprehensive stdlib | Stable | Mature |
| Higher-order functions | Stable | Mature |
| Pattern matching | Stable | Mature |
| FFI (C interop) | Stable | Mature |
| Module system | Stable | Mature |
| Concurrency | Stable | Mature |

---

## Further Reading

- **language-guide.md**: Comprehensive language tutorial
- **control-flow.md**: Detailed control flow documentation
- **builtins.md**: Complete builtin functions reference
- **scoping-guide.md**: Variable scoping and closures
- **examples/**: Browse example programs
