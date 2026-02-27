# Control Flow in Elle

A comprehensive guide to control flow constructs, exception handling, and the condition system in Elle.

## Table of Contents

1. [Conditionals](#conditionals)
2. [Imperative Loops](#imperative-loops)
3. [Functional Iteration](#functional-iteration)
4. [Exception Handling](#exception-handling)
5. [The Condition System](#the-condition-system)
6. [Control Flow Best Practices](#control-flow-best-practices)

---

## Conditionals

### if - The Basic Conditional

`if` is the fundamental conditional expression:

```lisp
(if condition then-branch else-branch)
```

The else branch is optional:

```lisp
(if condition then-branch)
```

**Examples:**

```lisp
(if (> 10 5)
  "10 is greater"
  "5 is greater")
⟹ "10 is greater"

(if #t
  (display "This prints"))
⟹ nil (else branch omitted, returns nil)

(if #f
  "not shown"
  "shown")
⟹ "shown"
```

### cond - Multiple Conditions

`cond` evaluates a series of conditions and returns the value of the first true branch:

```lisp
(cond
  (condition1 expression1)
  (condition2 expression2)
  ...
  (#t fallback-expression))
```

**Examples:**

```lisp
(var x 15)
(cond
  ((> x 20) "x is large")
  ((> x 10) "x is medium")
  ((> x 5)  "x is small")
  (#t       "x is tiny"))
⟹ "x is medium"
```

The final `(#t ...)` clause acts as a catch-all (equivalent to `else`):

```lisp
(cond
  ((number? x) "It's a number")
  ((string? x) "It's a string")
  (#t          "It's something else"))
```



### begin - Sequencing Expressions

`begin` sequences multiple expressions together, returning the value of the last. It does NOT create a new scope—bindings defined inside `begin` go into the enclosing scope.

```lisp
(begin
  expression1
  expression2
  ...
  final-expression)
```

**Examples:**

```lisp
(begin
  (display "First")
  (newline)
  (display "Second")
  (newline)
  (+ 2 2))
⟹ 4 (returns value of last expression)

(var result (begin
  (set x 10)
  (set y 20)
  (+ x y)))
result ⟹ 30
```

### block - Scoped Sequencing with break

`block` sequences expressions within a new lexical scope. Bindings don't leak out. You can optionally name a block and use `break` to exit early with a value.

```lisp
(block :optional-name
  expression1
  expression2
  ...
  final-expression)
```

**Examples:**

```lisp
; Simple block with scope isolation
(var x 100)
(block
  (var x 50)  ; Local to block, shadows outer x
  (display x))  ; Prints: 50
(display x)     ; Prints: 100

; Named block with break for early exit
(var result (block :search
  (for item (list 1 2 3 4 5)
    (if (= item 3)
      (break :search "found it")))
  "not found"))

result  ; ⟹ "found it"
```

---

## Imperative Loops

While Elle emphasizes functional iteration, it also provides imperative loop constructs for cases where they're more appropriate.

### while - Conditional Loop

`while` executes a body repeatedly as long as a condition is truthy:

```lisp
(while condition body)
```

**Examples:**

```lisp
(var counter 0)
(while (< counter 5)
  (begin
    (display counter)
    (newline)
    (set counter (+ counter 1))))
⟹ nil (prints 0 1 2 3 4)

(var x 100)
(while (> x 0)
  (set x (/ x 2)))
⟹ nil (x becomes 0 after repeated halving)
```

### forever - Infinite Loop

`forever` creates an infinite loop that must be exited via `break` or exception:

```lisp
(forever body...)
```

`forever` is syntactic sugar for `(while #t ...)`. It's useful for event loops or server loops that run until explicitly stopped.

**Examples:**

```lisp
; Simple infinite loop (would need break to exit)
(forever
  (display "Running...")
  (newline))

; Event loop pattern
(var running #t)
(forever
  (process-event)
  (if (not running)
    (break)))

; Multiple statements in body
(forever
  (display "Tick")
  (newline)
  (time/sleep 1)
  (if should-stop
    (break)))
```

---

## Functional Iteration

Elle emphasizes functional iteration patterns for processing collections. Use higher-order functions for data transformation:

### map - Transform Each Element

```lisp
(map function list)
```

**Examples:**

```lisp
(map (fn (x) (* x 2)) (list 1 2 3))
⟹ (2 4 6)

(map (fn (s) (string-upcase s)) (list "a" "b" "c"))
⟹ ("A" "B" "C")

(map abs (list -1 -2 3 -4))
⟹ (1 2 3 4)
```

### filter - Select Matching Elements

```lisp
(filter predicate list)
```

**Examples:**

```lisp
(filter (fn (x) (> x 2)) (list 1 2 3 4))
⟹ (3 4)

(filter even? (list 1 2 3 4 5 6))
⟹ (2 4 6)

(filter (fn (s) (> (length s) 3))
  (list "hi" "hello" "ok" "world"))
⟹ ("hello" "world")
```

### fold - Accumulate a Result

```lisp
(fold function initial-value list)
```

**Examples:**

```lisp
(fold (fn (acc x) (+ acc x)) 0 (list 1 2 3 4))
⟹ 10

(fold (fn (acc x) (cons x acc)) nil (list 1 2 3))
⟹ (3 2 1)

(fold (fn (acc s) (string-append acc " " s))
  "" (list "hello" "world"))
⟹ " hello world"
```

### Combining map, filter, and fold

Process data through multiple transformations:

```lisp
; Double all even numbers
(map (fn (x) (* x 2))
  (filter even? (list 1 2 3 4 5 6)))
⟹ (4 8 12)

; Sum of squares of positive numbers
(fold (fn (acc x) (+ acc (* x x)))
  0
  (filter (fn (x) (> x 0)) (list -2 3 -1 4 -5)))
⟹ 25 (3² + 4²)
```

---

## Exception Handling

### try-catch - Basic Error Handling

Use `try` to wrap potentially failing code and `catch` to handle exceptions:

```lisp
(try
  risky-expression
  (catch (e)
    handle-error))
```

**Examples:**

```lisp
(try
  (/ 10 0)
  (catch (e)
    (display "Division by zero!")
    (newline)))
```

The catch block receives the exception value:

```lisp
(var result (try
  (throw (exception "Bad input" (table "type" "validation")))
  (catch (e)
    (exception-message e))))

result ⟹ "Bad input"
```

### finally - Cleanup Code

`finally` executes regardless of success or failure:

```lisp
(try
  body
  (catch (e)
    handle-error)
  (finally
    cleanup-code))
```

Execution order:
1. body executes
2. If exception, catch block runs
3. finally always runs
4. Result from body (or catch) is returned

**Examples:**

```lisp
(var result (try
  (display "Body")
  (newline)
  42
  (catch (e)
    (display "Caught: ")
    (display e)
    (newline)
    0)
  (finally
    (display "Finally")
    (newline))))

result ⟹ 42
```

Output:
```
Body
Finally
```

With an exception:

```lisp
(try
  (throw "Error!")
  (catch (e)
    (display "Caught: ")
    (display e))
  (finally
    (display " - Cleanup"))))
```

Output:
```
Caught: Error! - Cleanup
```

### Creating and Throwing Exceptions

Use `exception` to create exception values:

```lisp
(var e (exception "Error message" data))
```

The data parameter can be any value (typically a table with context):

```lisp
(var e (exception "Database error"
  (table
    "code" 500
    "query" "SELECT * FROM users"
    "retry" #t)))
```

Use `throw` to raise an exception:

```lisp
(throw e)
(throw (exception "Simple error" nil))
(throw "String exceptions also work")
```

Extract information from exceptions:

```lisp
(var e (exception "Test" (table "x" 42)))
(exception-message e)  ⟹ "Test"
(exception-data e)     ⟹ #<table String("x")=42>
```

---

## The Condition System

> **Deprecated.** The condition system (`define-condition`, `catch-condition`,
> `condition-get`, `signal`) will be replaced by the fiber/signal model with
> `try`/`catch`/`finally` surface syntax. See `docs/fibers.md` for the new
> design. The primitives below still work but will be removed in a future
> release.

The condition system is a more sophisticated approach to error handling than simple exceptions. It allows defining custom signal types with handlers that can respond gracefully.

### Defining Conditions

Define a condition type with `define-condition`:

```lisp
(var-condition :condition-name
  (field1 "default-value-1")
  (field2 "default-value-2")
  ...)
```

**Examples:**

```lisp
(var-condition :validation-error
  (message "Validation failed")
  (field "unknown")
  (value nil))

(var-condition :network-error
  (message "Network failed")
  (url "")
  (status-code 0)
  (retry-count 0))
```

### Registering Handlers

Register a handler for a condition with `define-handler`:

```lisp
(var-handler :condition-name
  (fn (condition)
    handler-body))
```

Multiple handlers can be registered for the same condition - they're called in order:

```lisp
(var-handler :validation-error
  (fn (c)
    (display "Handler 1: ")
    (display (condition-get c 'message))
    (newline)))

(var-handler :validation-error
  (fn (c)
    (display "Handler 2: ")
    (display (condition-get c 'field))
    (newline)))
```

### Signaling Conditions

Use `signal` to trigger a condition:

```lisp
(signal :condition-name
  :field1 value1
  :field2 value2
  ...)
```

All registered handlers are called in the order they were defined:

```lisp
(signal :validation-error
  :message "Email format is invalid"
  :field "email"
  :value "not-an-email")
```

Output (if handlers are registered):
```
Handler 1: Email format is invalid
Handler 2: email
```

### Catching Conditions

Use `catch-condition` to intercept a specific condition:

```lisp
(catch-condition :condition-name
  (signal-body)
  (fn (condition)
    handler-body))
```

**Examples:**

```lisp
(catch-condition :validation-error
  (signal :validation-error
    :message "Invalid input"
    :field "username")
  (fn (c)
    (display "Caught and handled: ")
    (display (condition-get c 'message'))
    (newline)))
```

### Generic Condition Catching

Use `condition-catch` to handle any condition:

```lisp
(condition-catch
  (signal-body)
  (fn (condition-type condition-data)
    handler-body))
```

**Examples:**

```lisp
(condition-catch
  (begin
    (signal :validation-error :message "Bad email")
    (signal :network-error :message "No connection"))
  (fn (type data)
    (display "Caught ")
    (display type)
    (display ": ")
    (display (condition-get data 'message))
    (newline)))
```

### Condition Objects

Access condition fields with `condition-get`:

```lisp
(var c (signal :validation-error
  :message "Invalid"
  :field "email"))

(condition-get c 'message) ⟹ "Invalid"
(condition-get c 'field)   ⟹ "email"
```

### Practical Example: Input Validation

```lisp
; Define validation error condition
(var-condition :validation-error
  (message "Validation failed")
  (field "unknown")
  (constraint "unknown"))

; Register handlers
(var-handler :validation-error
  (fn (c)
    (display "VALIDATION ERROR: ")
    (display (condition-get c 'field))
    (display " - ")
    (display (condition-get c 'message))
    (newline)))

(var-handler :validation-error
  (fn (c)
    (display "  (constraint: ")
    (display (condition-get c 'constraint))
    (display ")")
    (newline)))

; Validation function
(def (validate-email email)
  (unless (string-contains? email "@")
    (signal :validation-error
      :message "Missing @ symbol"
      :field "email"
      :constraint "must contain @")))

; Use it
(validate-email "invalid-email")
```

Output:
```
VALIDATION ERROR: email - Missing @ symbol
  (constraint: must contain @)
```

---

## Control Flow Best Practices

### 1. Prefer `cond` for Multiple Conditions

Instead of nested `if`:

```lisp
; Good
(cond
  ((nil? x) "empty")
  ((> x 0) "positive")
  ((< x 0) "negative")
  (#t "zero"))

; Avoid
(if (nil? x)
  "empty"
  (if (> x 0)
    "positive"
    (if (< x 0)
      "negative"
      "zero")))
```

### 2. Use Functional Iteration for Data Processing

Prefer `map`, `filter`, and `fold` for processing collections:

```lisp
; Good: Clear intent, composable
(var doubled (map (fn (x) (* x 2)) (list 1 2 3)))
(var evens (filter even? (list 1 2 3 4 5 6)))
(var sum (fold (fn (acc x) (+ acc x)) 0 (list 1 2 3)))

; Less idiomatic: Manual recursion (still valid)
(def (sum-list lst)
  (if (nil? lst)
    0
    (+ (first lst) (sum-list (rest lst)))))
```

### 3. Use the Condition System for Expected Errors

Use exceptions for truly exceptional cases, conditions for expected error scenarios:

```lisp
; Good: Expected validation error
(var-condition :invalid-input
  (message "Input validation failed")
  (field "unknown"))

; Less good: Using exceptions for validation
(throw (exception "Input validation failed" nil))
```

### 4. Always Use try-finally for Resource Cleanup

If you open resources, ensure cleanup code runs:

```lisp
(try
  (process-file "data.txt")
  (finally
    (display "File processing complete")
    (newline)))
```

### 5. Separate Concerns with catch-condition

```lisp
; Good: Handle specific conditions
(catch-condition :network-error
  (fetch-data)
  (fn (c)
    (retry (condition-get c 'url))))

(catch-condition :validation-error
  (validate-input)
  (fn (c)
    (prompt-user-to-fix (condition-get c 'field))))

; Less good: Generic exception handling
(try
  (begin
    (fetch-data)
    (validate-input))
  (catch (e)
    ; Now we have to parse the error ourselves
    ...))
```

### 6. Combine map/filter for Complex Transformations

Chain functional operations for clarity:

```lisp
; Get square of all positive even numbers
(map (fn (x) (* x x))
  (filter even?
    (filter (fn (x) (> x 0))
      (list -2 1 2 3 -4 5 6))))
⟹ (4 36)
```

---

## Summary

| Construct | Use Case | Returns |
|-----------|----------|---------|
| `if` | Simple true/false choice | Any value |
| `cond` | Multiple conditions | First true branch value |
| `begin` | Sequence expressions (no scope) | Last expression value |
| `block` | Sequence expressions (with scope) | Last expression value |
| `break` | Exit block early with value | Block's return value |
| `while` | Conditional loop | nil |
| `forever` | Infinite loop (syntactic sugar for `while #t`) | nil |
| `map` | Transform each element | New list with transformed elements |
| `filter` | Select matching elements | New list with selected elements |
| `fold` | Accumulate a result | Final accumulated value |
| `try-catch` | Exception handling | Caught value or exception |
| `finally` | Cleanup code | Always executes |
| Condition system | Sophisticated error handling | Handler result |
