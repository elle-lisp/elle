# Elle Language Guide

A comprehensive guide to Elle's core language features, data types, control flow, and standard library.

## Table of Contents

1. [Introduction](#introduction)
2. [Basic Data Types](#basic-data-types)
3. [Variables and Bindings](#variables-and-bindings) (includes destructuring)
4. [Control Flow](#control-flow)
5. [Exception Handling](#exception-handling)
6. [The Condition System](#the-condition-system)
7. [Functions and Higher-Order Operations](#functions-and-higher-order-operations)
8. [Collections](#collections)
9. [Standard Library Overview](#standard-library-overview)
10. [Advanced Topics](#advanced-topics)

---

## Introduction

Elle is a Lisp dialect implemented in Rust with a focus on performance and expressiveness. It combines traditional Lisp features with modern conveniences, providing a clean and powerful programming environment.

### Key Features

- **First-class functions** and closures with proper lexical scoping
- **Rich data types** including lists, vectors, tables, and structs
- **Modern exception handling** with try-catch-finally and the condition system
- **Comprehensive standard library** for strings, math, file I/O, and more
- **Bytecode compilation** for efficient execution
- **FFI (Foreign Function Interface)** for calling C libraries

---

## Basic Data Types

Elle's type system provides a rich set of data types for different programming needs.

### Nil and Booleans

```lisp
nil          ; The null/empty value
#t           ; True
#f           ; False
```

Use `nil?` to test for null values and `boolean?` to test for boolean values.

```lisp
(nil? nil)       ⟹ #t
(nil? #f)        ⟹ #f
(boolean? #t)    ⟹ #t
(boolean? 42)    ⟹ #f
```

### Numbers

Elle supports both integers and floating-point numbers:

```lisp
42             ; Integer
3.14           ; Float
-17            ; Negative numbers
1.5e-3         ; Scientific notation
```

Use `number?` to test for numeric values. Use `type-of` to get the type name:

```lisp
(number? 42)     ⟹ #t
(number? "hello")⟹ #f
(type-of 42)     ⟹ :integer
(type-of 3.14)   ⟹ :float
```

### Strings

Strings are immutable sequences of characters:

```lisp
"hello"                    ; String literal
"line 1\nline 2"          ; Escape sequences: \n, \t, \", \\
(length "hello")          ⟹ 5
(string-append "hello" " " "world") ⟹ "hello world"
(substring "hello" 1 4)   ⟹ "ell"
```

Common string operations:

```lisp
(string-upcase "hello")           ⟹ "HELLO"
(string-downcase "HELLO")         ⟹ "hello"
(string-trim "  hello  ")         ⟹ "hello"
(string-contains? "hello" "ell")  ⟹ #t
(string-split "a,b,c" ",")       ⟹ ("a" "b" "c")
(string-replace "hello" "l" "L")  ⟹ "heLLo"
```

### Symbols and Keywords

Symbols are identifiers used in code:

```lisp
'symbol      ; Symbol (quote prevents evaluation)
:keyword     ; Keyword (self-evaluating)
(symbol? 'hello)  ⟹ #t
(symbol? "hello") ⟹ #f
```

### Lists

Linked lists are the fundamental collection type in Lisp:

```lisp
(list 1 2 3)          ⟹ (1 2 3)
(cons 1 (list 2 3))   ⟹ (1 2 3)
(first (list 1 2 3))  ⟹ 1
(rest (list 1 2 3))   ⟹ (2 3)
(length (list 1 2 3)) ⟹ 3
(append (list 1 2) (list 3 4)) ⟹ (1 2 3 4)
(reverse (list 1 2 3)) ⟹ (3 2 1)
(nth 1 (list 'a 'b 'c')) ⟹ b
```

### Arrays

Arrays are ordered collections optimized for random access:

```lisp
[1 2 3]             ; Array literal
(array 1 2 3)       ; Create array
(length [1 2 3]) ⟹ 3
(array-ref [1 2 3] 1) ⟹ 2
(array-set! [1 2 3] 0 99) ⟹ [99 2 3]
```

### Tables

Tables are mutable hash maps:

```lisp
(table)                    ; Empty table
(table "x" 10 "y" 20)     ; Table with entries
(get tbl "x")             ; Get value by key
(put tbl "z" 30)          ; Set/update key
(del tbl "x")             ; Delete key
(has-key? tbl "x")        ; Check if key exists
(keys tbl)                ; Get all keys
(values tbl)              ; Get all values
(length tbl)              ; Get number of entries
```

### Structs

Structs are immutable hash maps:

```lisp
(struct)                   ; Empty struct
(struct "x" 10 "y" 20)    ; Struct with entries
(struct-get s "x")        ; Get value by key (returns default-val if missing)
(struct-put s "z" 30)     ; Returns new struct with updated value
(struct-del s "x")        ; Returns new struct without key
(struct-has? s "x")       ; Check if key exists
(struct-keys s)           ; Get all keys
(struct-values s)         ; Get all values
(length s)                ; Get number of entries
```

### Type Checking Predicates

Elle provides predicate functions for type testing (all end with `?`):

```lisp
(nil? x)          ; Is x nil?
(boolean? x)      ; Is x a boolean?
(number? x)       ; Is x a number?
(symbol? x)       ; Is x a symbol?
(string? x)       ; Is x a string?
(pair? x)         ; Is x a cons cell?
(type-of x)       ; Get type name as keyword
```

---

## Variables and Bindings

### define - Global Definitions

Define creates a global binding:

```lisp
(var x 42)
(var message "Hello, Elle!")
(var my-list (list 1 2 3))

x ⟹ 42
message ⟹ "Hello, Elle!"
```

Once defined, you can modify a variable with `set!`:

```lisp
(set! x 100)
x ⟹ 100
```

### let - Local Bindings

`let` creates local variables scoped to an expression:

```lisp
(let ((x 10)
      (y 20))
  (+ x y))
⟹ 30
```

Variables bound by `let` shadow outer bindings within the expression:

```lisp
(var x 5)
(let ((x 10))
  x)    ⟹ 10
x      ⟹ 5
```

### let* - Sequential Bindings

`let*` allows later bindings to reference earlier ones:

```lisp
(let* ((x 5)
       (y (* x 2)))
  (+ x y))
⟹ 15
```

This is different from `let`, where all bindings are in parallel:

```lisp
(let ((x 5)
      (y (* x 2)))  ; x is still unbound here
  (+ x y))
⟹ Error: x is unbound
```

### Destructuring

Destructuring extracts values from lists and arrays into multiple bindings
in a single form. It works in `def`, `var`, `let`, `let*`, and function
parameters.

#### List Destructuring

Use a list pattern `(a b c)` on the left-hand side:

```lisp
(def (a b c) (list 1 2 3))
a ⟹ 1
b ⟹ 2
c ⟹ 3
```

Missing elements become `nil` (no error):

```lisp
(def (x y z) (list 1))
x ⟹ 1
y ⟹ nil
z ⟹ nil
```

Extra elements are silently ignored:

```lisp
(def (a b) (list 1 2 3 4))
a ⟹ 1
b ⟹ 2
```

#### Array Destructuring

Use brackets `[a b]` for arrays:

```lisp
(def [x y] [10 20])
x ⟹ 10
y ⟹ 20
```

#### Nested Destructuring

Patterns nest arbitrarily:

```lisp
(def ((a b) c) (list (list 1 2) 3))
a ⟹ 1
b ⟹ 2
c ⟹ 3

(def ([x y] z) (list [10 20] 30))
x ⟹ 10
y ⟹ 20
z ⟹ 30
```

#### Wildcard `_`

Use `_` to skip elements you don't need:

```lisp
(def (_ b _) (list 1 2 3))
b ⟹ 2

(def [_ y] [10 20])
y ⟹ 20
```

#### Rest Patterns `& name`

Use `& name` to collect remaining elements:

```lisp
; List rest — collects as a list
(def (head & tail) (list 1 2 3 4))
head ⟹ 1
tail ⟹ (2 3 4)

; Array rest — collects as an array
(def [first & others] [10 20 30])
first ⟹ 10
others ⟹ [20 30]

; Empty rest when all elements consumed
(def (a b & r) (list 1 2))
r ⟹ ()
```

#### Destructuring in `let` and `let*`

```lisp
(let (((a b) (list 10 20)))
  (+ a b))
⟹ 30

(let* (((a b) (list 1 2))
       (c (+ a b)))
  c)
⟹ 3
```

#### Destructuring in Function Parameters

Destructuring patterns in parameter lists extract values from arguments:

```lisp
(defn add-pair ((a b)) (+ a b))
(add-pair (list 3 4)) ⟹ 7

; Mix normal and destructured parameters
(defn weighted-sum (weight (a b))
  (+ (* weight a) (* weight b)))
(weighted-sum 2 (list 3 4)) ⟹ 14
```

#### Mutable Destructuring with `var`

`var` creates mutable bindings; `def` creates immutable ones:

```lisp
(var (a b) (list 1 2))
(set! a 100)
a ⟹ 100

(def (x y) (list 1 2))
(set! x 10) ⟹ Error: immutable binding
```

### defn - Named Function Shorthand

`defn` combines `def` and `fn`:

```lisp
(defn add (x y) (+ x y))
; equivalent to: (def add (fn (x y) (+ x y)))

(add 3 4) ⟹ 7
```

### set! - Mutation

`set!` updates an existing binding:

```lisp
(var counter 0)
(set! counter (+ counter 1))
counter ⟹ 1
```

---

## Control Flow

### if - Conditional Execution

`if` is the fundamental conditional:

```lisp
(if (> 10 5)
  "10 is greater"
  "5 is greater")
⟹ "10 is greater"

(if #f
  "this won't print"
  "this will print")
⟹ "this will print"
```

The else branch is optional:

```lisp
(if (> 5 10)
  (display "5 is greater"))
```

### cond - Multiple Conditions

`cond` is like a chain of if-else:

```lisp
(var x 15)
(cond
  ((> x 20) "x is large")
  ((> x 10) "x is medium")
  ((> x 5)  "x is small")
  (#t       "x is tiny"))
⟹ "x is medium"
```

The final clause `(#t ...)` acts as a catch-all.



### begin - Sequencing Expressions

`begin` sequences multiple expressions, returning the value of the last. It does NOT create a new scope—bindings defined inside `begin` go into the enclosing scope.

```lisp
(begin
  (display "First expression")
  (newline)
  (display "Second expression")
  (newline)
  (+ 2 2))
⟹ 4
```

Inside a function body, `begin` performs a two-pass analysis for mutual recursion, pre-creating `def`/`var` bindings.

### block - Scoped Sequencing

`block` sequences expressions within a new lexical scope. Bindings defined inside `block` don't leak out. You can optionally name a block and use `break` to exit early with a value.

```lisp
; Simple block
(block
  (var x 10)
  (display x))
; x is not accessible here

; Named block with break
(var result (block :my-block
  (var x 10)
  (if (> x 5)
    (break :my-block "early exit"))
  "normal exit"))

result  ; ⟹ "early exit"
```

`break` exits the innermost (or named) block, returning a value. Syntax: `(break)`, `(break val)`, `(break :name)`, `(break :name val)`. `break` is validated at compile time—it must be inside a block and cannot cross function boundaries.

### Functional Iteration (map, filter, fold)

Elle uses functional iteration rather than imperative loops. Use higher-order functions to process collections:

```lisp
; Process each element
(map (fn (x) (* x 2)) (list 1 2 3))
⟹ (2 4 6)

; Select matching elements
(filter (fn (x) (> x 2)) (list 1 2 3 4))
⟹ (3 4)

; Accumulate a result
(fold (fn (acc x) (+ acc x)) 0 (list 1 2 3 4))
⟹ 10
```

See the [Higher-Order Functions](#functions-and-higher-order-operations) section for more details.

---

## Exception Handling

### try-catch - Basic Error Handling

`try` evaluates code and `catch` handles exceptions:

```lisp
(try
  (/ 10 0)
  (catch (e)
    (display "Error caught!")
    (newline)))
```

The catch block receives the error value.

### finally - Cleanup Code

`finally` runs regardless of success or failure:

```lisp
(try
  (display "Attempting operation")
  (newline)
  (throw "Something went wrong")
  (catch (e)
    (display "Caught: ")
    (display e)
    (newline))
  (finally
    (display "Cleanup code runs here")
    (newline)))
```

Output:
```
Attempting operation
Caught: Something went wrong
Cleanup code runs here
```

### Creating and Throwing Exceptions

Use `exception` to create exception values and `throw` to raise them:

```lisp
(var my-error (exception "Invalid input" (table "code" 42)))
(throw my-error)

; Or throw directly:
(throw (exception "Something failed" nil))
```

Get information from exceptions:

```lisp
(var e (exception "Test error" (table "context" "validation")))
(exception-message e) ⟹ "Test error"
(exception-data e)    ⟹ #<table String("context")="validation">
```

---

## The Condition System

> **Deprecated.** The condition system described below will be replaced by
> the fiber/signal model with `try`/`catch`/`finally` surface syntax. See
> `docs/FIBERS.md`. These primitives still work but will be removed.

Elle provides a modern condition system for sophisticated error handling and signaling.

### Signals and Handlers

The condition system allows defining custom signal types with handlers:

```lisp
(var-condition :validation-error
  (message "Validation failed")
  (field "The field that failed"))

(var-handler :validation-error
  (fn (condition)
    (display "Validation error in ")
    (display (condition-get condition 'field))
    (display ": ")
    (display (condition-get condition 'message))
    (newline)))
```

### Signaling Conditions

Use `signal` to trigger a condition:

```lisp
(signal :validation-error
  :message "Email is invalid"
  :field "email")
```

The system will invoke registered handlers in order.

### Catching Conditions

Use `catch-condition` to intercept specific conditions:

```lisp
(catch-condition :validation-error
  (signal :validation-error
    :message "Invalid format"
    :field "username"))
  (fn (condition)
    (display "Handled validation error")
    (newline)))
```

### Multiple Handlers

Register multiple handlers for same signal - they're called in order:

```lisp
(var-handler :validation-error
  (fn (c) (display "Handler 1") (newline)))

(var-handler :validation-error
  (fn (c) (display "Handler 2") (newline)))

(signal :validation-error
  :message "Test"
  :field "test")
```

Output:
```
Handler 1
Handler 2
```

### Catch-All with condition-catch

Catch all conditions with a generic handler:

```lisp
(condition-catch
  (signal :some-error :data 42)
  (fn (condition-type condition-data)
    (display "Caught: ")
    (display condition-type)
    (newline)))
```

---

## Functions and Higher-Order Operations

### Defining Functions

Functions are defined with `fn` and named with `defn`:

```lisp
; Anonymous function
(fn (x y) (+ x y))

; Named function with defn (preferred)
(defn add (x y) (+ x y))

; Equivalent long form
(def add (fn (x y) (+ x y)))

(add 3 4) ⟹ 7
```

`defn` supports destructured parameters:

```lisp
(defn sum-pair ((a b)) (+ a b))
(sum-pair (list 3 4)) ⟹ 7
```

Note: `lambda` is available as an alias for `fn`.

### Closures

Functions close over their definition environment:

```lisp
(defn make-adder (n)
  (fn (x) (+ x n)))

(var add-5 (make-adder 5))
(add-5 10) ⟹ 15
(add-5 20) ⟹ 25
```

### Higher-Order Functions

#### map

`map` applies a function to each element of a list:

```lisp
(map (fn (x) (* x 2)) (list 1 2 3))
⟹ (2 4 6)

(map (fn (x) (> x 2)) (list 1 2 3 4))
⟹ (#f #f #t #t)
```

#### filter

`filter` selects elements that satisfy a predicate:

```lisp
(filter (fn (x) (> x 2)) (list 1 2 3 4))
⟹ (3 4)

(filter (fn (x) (even? x)) (list 1 2 3 4 5 6))
⟹ (2 4 6)
```

#### fold

`fold` (also called reduce) accumulates a result:

```lisp
(fold (fn (acc x) (+ acc x)) 0 (list 1 2 3 4))
⟹ 10

(fold (fn (acc x) (cons x acc)) nil (list 1 2 3))
⟹ (3 2 1)
```

### apply

`apply` calls a function with a list of arguments:

```lisp
(apply + (list 1 2 3))
⟹ 6

(defn add-three (x y z) (+ x y z))
(apply add-three (list 10 20 30))
⟹ 60
```

---

## Collections

### Working with Lists

Common list operations:

```lisp
; Construction
(list 1 2 3)           ⟹ (1 2 3)
(cons 1 (list 2 3))    ⟹ (1 2 3)

; Access
(first (list 1 2 3))   ⟹ 1
(rest (list 1 2 3))    ⟹ (2 3)
(nth 1 (list 'a 'b 'c'))⟹ b
(last (list 1 2 3))    ⟹ 3

; Modification (non-destructive)
(append (list 1 2) (list 3 4)) ⟹ (1 2 3 4)
(reverse (list 1 2 3)) ⟹ (3 2 1)
(take 2 (list 1 2 3 4)) ⟹ (1 2)
(drop 2 (list 1 2 3 4)) ⟹ (3 4)

; Querying
(length (list 1 2 3))  ⟹ 3
(pair? (list 1 2))     ⟹ #t
(nil? nil)             ⟹ #t
```

### Working with Tables

Tables are mutable:

```lisp
; Creation
(var t (table))
(var t2 (table "x" 10 "y" 20))

; Retrieval
(get t2 "x")           ⟹ 10
(get t2 "z" 99)        ⟹ 99 (with default)

; Modification
(put t2 "x" 15)        ; Modifies in place
(del t2 "y")           ; Delete key

; Querying
(has-key? t2 "x")      ⟹ #t
(keys t2)              ⟹ ("x" "y")
(values t2)            ⟹ (15 20)
(length t2)            ⟹ 2
```

### Working with Structs

Structs are immutable:

```lisp
; Creation
(var s (struct "a" 1 "b" 2))

; Retrieval (get with optional default)
(struct-get s "a")        ⟹ 1
(struct-get s "z" "N/A")  ⟹ "N/A"

; "Modification" (returns new struct)
(var s2 (struct-put s "c" 3))
(struct-get s "c")        ⟹ Error: key not found
(struct-get s2 "c")       ⟹ 3

; Deletion (returns new struct)
(var s3 (struct-del s2 "b"))
(struct-has? s3 "b")      ⟹ #f

; Querying
(struct-keys s)           ⟹ ("a" "b")
(struct-values s)         ⟹ (1 2)
(length s)                ⟹ 2
```

---

## Standard Library Overview

### Arithmetic

```lisp
(+ 1 2 3)          ⟹ 6
(- 10 3)           ⟹ 7
(* 2 3 4)          ⟹ 24
(/ 20 4)           ⟹ 5
(mod 10 3)         ⟹ 1
(rem 10 3)         ⟹ 1
(abs -5)           ⟹ 5
(min 3 1 4 1 5)    ⟹ 1
(max 3 1 4 1 5)    ⟹ 5
(even? 4)          ⟹ #t
(odd? 3)           ⟹ #t
```

### Math Functions

```lisp
(sqrt 16)          ⟹ 4.0
(pow 2 8)          ⟹ 256
(exp 1)            ⟹ 2.71828...
(log 2.71828)      ⟹ 1.0
(sin 0)            ⟹ 0.0
(cos 0)            ⟹ 1.0
(tan pi/4)         ⟹ 1.0
(floor 3.7)        ⟹ 3
(ceil 3.2)         ⟹ 4
(round 3.5)        ⟹ 4
pi                 ⟹ 3.14159...
e                  ⟹ 2.71828...
```

### String Operations

```lisp
(length "hello")                   ⟹ 5
(string-append "hello" " " "world")⟹ "hello world"
(string-upcase "hello")            ⟹ "HELLO"
(string-downcase "HELLO")          ⟹ "hello"
(substring "hello" 1 4)            ⟹ "ell"
(string-index "hello" "ll")        ⟹ 2
(char-at "hello" 0)                ⟹ "h"
(string-split "a,b,c" ",")         ⟹ ("a" "b" "c")
(string-replace "hello" "l" "L")   ⟹ "heLLo"
(string-trim "  hello  ")          ⟹ "hello"
(string-contains? "hello" "ell")   ⟹ #t
(string-starts-with? "hello" "he") ⟹ #t
(string-ends-with? "hello" "lo")   ⟹ #t
(string-join (list "a" "b" "c") "-") ⟹ "a-b-c"
(int "42")                         ⟹ 42
(float "3.14")                     ⟹ 3.14
(string 42)                        ⟹ "42"
(number->string 42)                ⟹ "42"
```

### File I/O

```lisp
(slurp "path/to/file.txt")         ; Read entire file as string
(spit "path/to/file.txt" "content"); Write to file (overwrites)
(append-file "path/to/file.txt" "\nmore") ; Append to file
(file-exists? "path/to/file.txt")  ; Check if file exists
(file? "path/to/file.txt")         ; Check if it's a regular file
(directory? "path/to/dir")         ; Check if it's a directory
(file-size "path/to/file.txt")     ; Get file size in bytes
(delete-file "path/to/file.txt")   ; Delete file
(delete-directory "path/to/dir")   ; Delete empty directory
(create-directory "path/to/dir")   ; Create single directory
(create-directory-all "path/to/deeply/nested/dir") ; Create with parents
(list-directory "path/to/dir")     ; List directory contents
(read-lines "path/to/file.txt")    ; Read file as list of lines
(file-name "/path/to/file.txt")    ⟹ "file.txt"
(file-extension "/path/to/file.txt") ⟹ ".txt"
(parent-directory "/path/to/file.txt") ⟹ "/path/to"
(current-directory)                ; Get working directory
(change-directory "path")          ; Change working directory
(absolute-path "file.txt")         ; Get absolute path
(join-path "dir1" "dir2" "file.txt") ⟹ "dir1/dir2/file.txt"
(copy-file "src.txt" "dst.txt")    ; Copy file
(rename-file "old.txt" "new.txt")  ; Rename/move file
```

### JSON Operations

```lisp
(json-parse "{\"x\": 42, \"y\": \"hello\"}")
⟹ #<table String("x")=42 String("y")="hello">

(json-serialize (table "x" 42 "y" "hello"))
⟹ "{\"x\":42,\"y\":\"hello\"}"

(json-serialize-pretty (table "x" 42 "y" "hello"))
⟹ "{\n  \"x\": 42,\n  \"y\": \"hello\"\n}"
```

### Type Conversions

```lisp
(int "42")        ⟹ 42
(float "3.14")    ⟹ 3.14
(string 42)       ⟹ "42"
(string 3.14)     ⟹ "3.14"
```

### Concurrency

```lisp
(spawn (fn () (display "Hello from thread") (newline)))
; Creates a new thread and runs the function

(var t (spawn (fn () (+ 2 2))))
(join t)          ; Wait for thread to complete, returns its result

(time/sleep 1)         ; Sleep for 1 second
(time/sleep 0.5)       ; Sleep for 500 milliseconds

(current-thread-id) ; Get ID of current thread
```

---

## Advanced Topics

### Quoting and Quasiquoting

Quote prevents evaluation:

```lisp
'(+ 1 2)       ⟹ (+ 1 2)    ; Not evaluated
(+ 1 2)        ⟹ 3          ; Evaluated

'(a b c)       ⟹ (a b c)    ; Symbols, not evaluated
```

Quasiquote allows selective evaluation with unquote:

```lisp
(var x 5)
`(the value is ,x)  ⟹ (the value is 5)
`(a ,x b)           ⟹ (a 5 b)
```

### Pattern Matching

Pattern matching destructures data:

```lisp
(match (list 1 2 3)
  ((list a b c) (+ a b c))
  (_ "no match"))
⟹ 6

(match (table "x" 10 "y" 20)
  ((table :x x :y y) (+ x y))
  (_ "no match"))
⟹ 30
```

### Macros

Macros transform code at compile time:

```lisp
(var-macro when
  (fn (test body)
    `(if ,test ,body nil)))

(when (> 10 5)
  (display "True!")
  (newline))
```

### Module System

Load external files as modules:

```lisp
(import-file "lib/helpers.elle")

; Add custom search paths:
(add-module-path "/opt/elle-libs")
```

### Closures and Variable Capture

Functions capture their definition environment:

```lisp
(defn make-counter ()
  (var count 0)
  (fn ()
    (set! count (+ count 1))
    count))

(var c1 (make-counter))
(c1) ⟹ 1
(c1) ⟹ 2
(c1) ⟹ 3
```

Each closure has its own captured variables:

```lisp
(var c2 (make-counter))
(c2) ⟹ 1
(c1) ⟹ 4
(c2) ⟹ 2
```

---

## Best Practices

### Use let for Local Scope

Prefer `let` and `let*` over global definitions for local work:

```lisp
; Good
(let ((x 10) (y 20))
  (+ x y))

; Avoid when possible
(var x 10)
(var y 20)
(+ x y)
```

### Prefer Immutable Operations

Use `struct` instead of `table` when you don't need mutation:

```lisp
; Better for functional style
(var-constant user (struct "id" 1 "name" "Alice"))

; Use table only when mutation is needed
(var cache (table))
(put cache "key" "value")
```

### Leverage Higher-Order Functions

Use `map`, `filter`, and `fold` for clear data transformations:

```lisp
; Clear intent
(map (fn (x) (+ x 1)) (list 1 2 3))

; More concise with existing functions
(map abs (list -1 -2 -3))
```

### Handle Errors Explicitly

Use the condition system for expected error cases:

```lisp
(catch-condition :validation-error
  (validate-user-input username)
  (fn (c)
    (display "Invalid: ")
    (display (condition-get c 'message))
    (newline)))
```

---

## Further Reading

- **BUILTINS.md**: Comprehensive reference of all built-in primitives
- **SCOPING_GUIDE.md**: Detailed explanation of variable scoping rules
- **examples/**: Browse example programs for common patterns
