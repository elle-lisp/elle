# Variable Scoping in Elle Lisp

## Overview

Elle Lisp implements proper lexical scoping with support for global, function, block, loop, and let-binding scopes. This guide explains how scoping works and how to use it effectively.

## Scope Types

### 1. Global Scope

Variables defined at the top level are global and accessible everywhere.

```lisp
(var global-x 100)

(var my-function (lambda ()
  (display global-x)))  ; Can access global-x

(my-function)  ; Prints: 100
```

### 2. Function Scope (Parameters & Captures)

Function parameters are local to the function and shadow outer variables.

```lisp
(var x 100)

(var add-one (lambda (x)
  (+ x 1)))  ; x is parameter, shadows global x

(add-one 5)   ; Returns 6
x             ; Still 100
```

### 3. Block Scope

Block scopes are created by `block` expressions. The `block` form creates a new lexical scope where bindings don't leak out.

```lisp
(var x 100)

(block
  (var x 50)  ; Block-scoped, shadows outer x
  (display x))   ; Prints: 50

(display x)      ; Prints: 100
```

You can optionally name a block and use `break` to exit early with a value:

```lisp
(var result (block :my-block
  (var x 10)
  (if (> x 5)
    (break :my-block "early exit"))
  "normal exit"))

result  ; ⟹ "early exit"
```

### 4. Loop Scope

While and for loops create their own scope for loop variables.

```lisp
(var counter 0)

(while (< counter 3)
  (begin
    (display counter)
    (set counter (+ counter 1))))

; counter exists globally and is modified
; The loop body has loop scope
```

**Important**: Loop variables don't leak out of loops:

```lisp
(for i (list 1 2 3)
  (display i))

; i is NOT accessible here - loop scoped!
```

### 5. Let-Binding Scope

Let-bindings create local variables with scope isolation.

```lisp
(let ((x 5)
      (y 10))
  (display (+ x y)))  ; x and y are local

; x and y are NOT accessible here
```

## Scope Chain Lookup

When a variable is referenced, Elle searches for it in this order:

1. **Current scope** - Check the current block/loop/let scope
2. **Parent scopes** - Walk up the scope chain to parent blocks/functions
3. **Global scope** - Finally check global variables
4. **Error** - If not found anywhere

```lisp
(var global-x 1)

(var outer-function (lambda (param-x)
  (let ((let-x 5))
    (display (+ global-x param-x let-x)))))
    ; Lookup order: let-x (found) → param-x (found) → global-x (found)

(outer-function 2)  ; Prints: 1 + 2 + 5 = 8
```

## Variable Shadowing

Inner scopes can shadow (hide) outer scope variables:

```lisp
(var x 100)

(lambda (x)  ; Parameter x shadows global x
  (+ x 1))   ; References parameter x, not global

(call-lambda-with 5)  ; Returns 6, not 101
```

This is usually clear when variables have descriptive names:

```lisp
(var total 1000)

(lambda (total)  ; OK: parameter total shadows global total
  (+ total 100))
```

But can be confusing with poor naming:

```lisp
(var x 100)

(lambda (x)  ; Confusing: hides outer x
  (let ((x 5))  ; Even more confusing!
    (+ x 1)))
```

## Variable Modification (set!)

The `set` operator modifies existing variables:

```lisp
(var counter 0)

(lambda ()
  (set counter (+ counter 1))  ; Modifies global counter
  counter)
```

`set` searches the scope chain to find where a variable is defined:

```lisp
(var outer-var 100)

(lambda ()
  (set outer-var 200)  ; Modifies outer-var in global scope
)
```

## Let-binding Scope Details

### Let (Parallel Binding)

In regular `let`, binding expressions cannot reference previous bindings:

```lisp
(let ((x 5)
      (y 10))   ; y cannot reference x
  (+ x y))
```

This is valid:
```lisp
(let ((x 5)
      (y (+ 2 3)))   ; (2 + 3) is evaluated independently
  (+ x y))
```

### Let* (Sequential Binding)

`let*` allows each binding to reference previous bindings:

```lisp
(let* ((x 5)
       (y (+ x 1))    ; Can reference x!
       (z (+ y 1)))   ; Can reference y!
  (+ x y z))
; Result: 5 + 6 + 7 = 18
```

## Common Scoping Patterns

### Pattern 1: Temporary Variables

Use let-bindings for temporary calculations:

```lisp
(let ((temp-result (* x y))
      (temp-sum (+ a b)))
  (+ temp-result temp-sum))
```

### Pattern 2: Function Factories

Create functions with captured variables:

```lisp
(var make-multiplier (lambda (factor)
  (lambda (x)
    (* x factor))))

(var double (make-multiplier 2))
(var triple (make-multiplier 3))

(double 5)   ; Returns 10
(triple 5)   ; Returns 15
```

### Pattern 3: Loop Accumulation

Use a global or let-bound accumulator with loops:

```lisp
(let ((sum 0))
  (for item (list 1 2 3 4 5)
    (set sum (+ sum item)))
  sum)  ; Returns 15
```

### Pattern 4: Nested Functions

Inner functions access outer scope:

```lisp
(var make-adder (lambda (base)
  (lambda (x)
    (+ base x))))  ; Can access base from outer scope

(var add-10 (make-adder 10))
(add-10 5)  ; Returns 15
```

## Scope Errors

### Undefined Variable

```lisp
(display undefined-var)  ; ERROR: Undefined global variable
```

**Fix**: Define the variable first:
```lisp
(var undefined-var 42)
(display undefined-var)  ; OK
```

### Variable Out of Scope

```lisp
(let ((x 5))
  (display x))  ; OK - inside let

(display x)     ; ERROR: x not defined
```

**Fix**: Use the variable inside its scope:
```lisp
(let ((x 5))
  (display x))
```

### Accessing Loop Variables Outside Loop

```lisp
(for i (list 1 2 3)
  (display i))

(display i)  ; ERROR: i not defined (loop-scoped)
```

**Fix**: Use loop-bound variables only inside the loop:
```lisp
(for i (list 1 2 3)
  (display i))

; Process results outside loop with global accumulator
(let ((sum 0))
  (for i (list 1 2 3)
    (set sum (+ sum i)))
  (display sum))
```

## Best Practices

1. **Use descriptive names** to avoid shadowing confusion
2. **Keep scope narrow** - use let-bindings instead of global variables
3. **Avoid unnecessary shadowing** - don't reuse variable names confusingly
4. **Document captured variables** - make it clear which outer variables a function uses
5. **Test scope edge cases** - ensure your scoping works as expected

## Implementation Details

Elle's scope management uses a scope stack at runtime:

- **Global scope**: Always at the bottom of the stack
- **Function scopes**: Created when functions are called
- **Block/Loop/Let scopes**: Created by PushScope, destroyed by PopScope
- **Lookup**: Traverses stack from current scope to global
- **Modification**: set! walks the stack to find where variable is defined

## See Also

- `examples/scope_management.lisp` - Working examples of all scoping features
- `examples/scope_explained.lisp` - Comprehensive scope demonstration
- Source code: `src/vm/scope.rs` - Runtime scope implementation
- Source code: `src/compiler/compile.rs` - Scope compilation
