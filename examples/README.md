# Elle Examples - Assertion-Based Contracts

This directory contains examples demonstrating Elle Lisp features. Each example
is a **self-verifying contract** that exits with code 0 on success and code 1
on failure.

## What Are Assertion-Based Contracts?

Each example:
- Documents the intended behavior of a feature
- Demonstrates the feature with clear code
- **Verifies** the behavior with assertions
- Exits with code 0 if all assertions pass
- Exits with code 1 if any assertion fails

This means if the implementation changes and violates the documented behavior,
the example will crash. Examples are executable specifications.

## Running Examples

To run a single example:
```bash
./target/debug/elle examples/hello.lisp
echo $?  # Should be 0 if successful
```

## Assertion Helpers

All examples define these helper functions locally:

- `assert-eq(actual, expected, msg)` - Assert two values are equal
  - Uses `eq?` for symbols, `=` for numbers
  - Displays error with expected vs actual
  - Exits with code 1 on failure

- `assert-true(val, msg)` - Assert value is #t
  - Exits with code 1 if not true

- `assert-false(val, msg)` - Assert value is #f
  - Exits with code 1 if not false

- `assert-list-eq(actual, expected, msg)` - Assert lists are equal
  - Compares element by element
  - Exits with code 1 if not equal

See `assertions.lisp` for the standard assertion library documentation.

## Examples by Category

### Foundations (Tier 0)
- **assertions.lisp** - Standard assertion library documentation and reference

### Basics (Tier 1)
- **hello.lisp** - Simple greeting, basic output, and shebang script examples
- **math-and-logic.lisp** - Arithmetic, math functions, predicates (even?/odd?), and logical operations (and, or, not, xor)
- **lists-and-arrays.lisp** - List operations (first, rest, cons), array operations (array-ref, array-set!), and comparisons
- **types.lisp** - All atomic types (keywords, symbols, numbers, strings, booleans, nil), type predicates, type conversions, and mutable storage with boxes

### Computation (Tier 2)
- **closures.lisp** - Lambda functions, closures, mutable captures, and comprehensive recursion patterns (self-recursion, mutual recursion, tail recursion, nested definitions, multi-way recursion)
- **scope-and-binding.lisp** - Binding forms (let, let*, function parameters), lexical scoping, shadowing, scope management, closures, nested functions, begin form for sequencing, block form for scoped sequencing with break
- **control-flow.lisp** - Conditionals (cond), loops (for/each, while, forever), and pattern matching (match with binding)
- **string-operations.lisp** - String manipulation and String Module

### Data Structures (Tier 3)
- **tables-and-structs.lisp** - Table and struct operations with syntactic sugar ({} and @{} syntax)
- **json.lisp** - JSON parsing and encoding

### Advanced (Tier 4)
- **concurrency.lisp** - Thread creation and joining with spawn/join
- **io.lisp** - File reading, writing, and directory operations
- **modules.lisp** - Module system (import-file, add-module-path, organization patterns, file-based module integration)

### Miscellaneous (Tier 5)
- **syntax-sugar.lisp** - Syntactic sugar features (thread-first/thread-last operators)
- **coroutines.lisp** - Coroutine creation, resumption, and advanced patterns
- **meta-programming.lisp** - Macros, meta-programming, quasiquote/unquote, gensym, type-of
- **higher-order-functions.lisp** - Functions that operate on functions
- **debugging-profiling.lisp** - Debugging and profiling tools

## Adding Assertions to New Examples

When creating a new example:

1. **Define assertion helpers** at the top:
```lisp
(def assert-eq (fn (actual expected msg)
  (let ((matches
    (if (symbol? expected)
        (eq? actual expected)
        (= actual expected))))
    (if matches
        #t
        (begin
          (display "FAIL: ")
          (display msg)
          (newline)
          (exit 1))))))
```

2. **Add assertions** after each example:
```lisp
(var result (+ 1 2))
(assert-eq result 3 "1 + 2 = 3")
```

3. **Verify it exits with code 0**:
```bash
./target/debug/elle examples/myexample.lisp
echo $?
```

## Statistics

- **Total Examples**: 21 (consolidated from 47)
  - 1 assertions library (assertions.lisp)
  - 20 feature examples
- **Total Assertions**: 632+
- **Assertion Pass Rate**: 100% (for passing examples)
- **Coverage**: All major Elle features
- **Latest Merge**: loops-and-iteration.lisp + pattern-matching.lisp → control-flow.lisp (42 assertions in control-flow.lisp)

## Consolidation Summary

### Final Consolidations (Session 10 - Latest Round)

- `loops-and-iteration.lisp` + `pattern-matching.lisp` → **control-flow.lisp**
  - Merged loop patterns and pattern matching into unified control-flow file
  - Part 1: Conditionals with cond (grade assignment, sign detection, age-based status)
  - Part 2: Loops and iteration (list operations, cons, first/rest, length, reverse, take/drop, nth/last, append, nested lists, forever loops)
  - Part 3: Pattern matching (literal matching, string matching, wildcards, nil patterns, list patterns, nested lists, computed results)
  - 42 assertions covering all control flow patterns

- `scope-and-binding.lisp` enhanced with **begin and block form sections**
   - Part 0: The begin Form - Sequencing (No Scope)
   - Part 0.5: The block Form - Scoped Sequencing
   - Demonstrates begin as sequencing without scope (variables leak)
   - Demonstrates block as scoped sequencing (variables contained)
   - Shows named blocks and break for early exit
   - Multiple assertions for begin and block form behavior

### Previous Consolidations (Session 9)

- `mutual-recursion.lisp` → **closures.lisp**
  - Merged mutual recursion patterns into closures.lisp
  - Added countdown with two functions (count-down-a, count-down-b)
  - Added string processing with mutual recursion
  - Added factorial with helper function (accumulator pattern)
  - Added three-way mutual recursion (func-a, func-b, func-c)
  - Added filtering with mutual recursion (separate-numbers)
  - Added alternating pattern (step-x, step-y)
  - Comprehensive recursion coverage: self-recursion, mutual recursion, tail recursion, nested definitions
  - 93 assertions covering all recursion patterns

### Previous Consolidations (Session 8)

- `types.lisp` + `mutable-storage.lisp` → **types.lisp**
  - Comprehensive atomic types coverage: keywords (`:keyword`), symbols (`'symbol`), numbers, strings (`"string"`), booleans (`#t`, `#f`), nil
  - Type predicates: nil?, pair?, list?, number?, symbol?, string?, boolean?
  - Type conversions: int, float, string, string->int, string->float, number->string, symbol->string, any->string
  - Round-trip conversions and conversion chains
  - Mutable storage with boxes: box, unbox, box-set!, box?
  - Box use cases: counters, accumulators, mutable state
  - Boxes vs immutable structures comparison
  - 395+ assertions covering all type and mutable storage operations

### Previous Consolidations (Session 7)

- `atoms.lisp` + `type-checking.lisp` → **types.lisp** (before mutable-storage merge)
  - Comprehensive atomic types coverage: keywords (`:keyword`), symbols (`'symbol`), numbers, strings (`"string"`), booleans (`#t`, `#f`), nil
  - Type predicates: nil?, pair?, list?, number?, symbol?, string?, boolean?
  - Type conversions: int, float, string, string->int, string->float, number->string, symbol->string, any->string
  - Round-trip conversions and conversion chains
  - 147 assertions covering all type operations

### Previous Consolidations (Session 6)

- `keywords.lisp` → renamed to **atoms.lisp**
  - Expanded to cover all atomic types in Elle
  - Keywords (`:keyword`), symbols (`'symbol`), numbers, strings (`"string"`), booleans (`#t`, `#f`), nil
  - Type checking and operations on each atom type
  - Mixed atoms in collections

- `file-io.lisp` → renamed to **io.lisp**
  - File reading and writing operations
  - Directory operations and path manipulation
  - File information and properties

- `list-operations.lisp` + `array-operations.lisp` → **lists-and-arrays.lisp**
   - List operations (first, rest, cons) and List Module
   - Array creation, access (array-ref), and mutation (array-set!)
   - Arrays vs lists comparison
   - Polymorphic length function

- `math-operations.lisp` + `logic-operations.lisp` → **math-and-logic.lisp**
  - Basic arithmetic (+, -, *, /, mod)
  - Math functions (sqrt, pow, sin, cos, floor, ceil)
  - Mathematical constants (pi, e)
  - Arithmetic predicates (even?, odd?)
  - Logical operations (not, and, or, xor)
  - Practical examples and predicate combinations

### Previous Consolidations (Session 5)

- `modules.lisp` + `file-modules.lisp` → **modules.lisp**
  - Module path management and import-file functionality
  - File-based module integration and idempotent loading
  - Module organization, composition, namespacing, dependencies
  - Module re-export, initialization, and testing patterns

- `recursion.lisp` → merged into **closures.lisp**
  - Self-recursion (Fibonacci, factorial)
  - Mutual recursion (even/odd predicates)
  - Tail recursion with accumulators
  - Recursion with nested definitions
  - Combined with lambda expressions and closure patterns

- `macros.lisp` → renamed to **meta-programming.lisp**
  - Macro definition and expansion
  - Meta-programming patterns and code generation
  - gensym and type-of functionality
  - Macro composition and symbol manipulation

- `binding.lisp` + `scope.lisp` → **scope-and-binding.lisp**
  - Binding forms (let, let*, function parameters)
  - Lexical scoping and scope isolation
  - Shadowing rules and parameter shadowing
  - Global scope, function scope, let-bindings
  - Loop variable scoping and closure captures
  - Nested functions and scope hierarchy
  - Best practices and common mistakes

- `quasiquote-unquote.lisp` → merged into **meta-programming.lisp**
  - Quote and quasiquote syntax
  - Unquote for selective evaluation
  - Code templates and metaprogramming patterns
  - Nested quotes and mixed quoted/unquoted elements

### Previous Consolidations (Session 4)

- `coroutines.lisp` + `coroutines-advanced.lisp` → **coroutines.lisp**
  - Basic coroutine creation and yield
  - Multiple yields and closure captures
  - Interleaved coroutines and value tracking
  - Quoted symbols and expression evaluation
  - Nested coroutines and generator patterns
  - Advanced features: range/fibonacci generators
  - Multiple independent coroutines and completion detection

- `type-conversion.lisp` + `type-checking.lisp` → **type-checking.lisp**
  - Type predicates (nil?, pair?, list?, number?, symbol?, string?, boolean?)
  - Type conversion functions (int, float, string)
  - String parsing (string->int, string->float)
  - Number/symbol/any to string conversions
  - Round-trip conversions and conversion chains

- `threading-operators.lisp` → renamed to **syntax-sugar.lisp**
  - Thread-first (->) operator
  - Thread-last (->>) operator
  - Comparison of threading operators
  - Practical threading examples

### Previous Consolidations

- `scope-explained.lisp` + `scope-management.lisp` → **scope.lisp**
  - Global scope, function scope, let-bindings
  - Shadowing, loop scoping, closures
  - Nested functions, scope hierarchy
  - Best practices and common mistakes

- `math.lisp` + `arithmetic.lisp` + `arithmetic-predicates.lisp` → **math-operations.lisp**
  - Basic arithmetic (+, -, *, /, mod)
  - Math functions (sqrt, pow, sin, cos, floor, ceil)
  - Mathematical constants (pi, e)
  - Arithmetic predicates (even?, odd?)
  - Filtering, counting, and sum operations with predicates

### Files Renamed
- `ffi-basics.lisp` → **ffi.lisp**
- `threading-operators.lisp` → **syntax-sugar.lisp**

### Files Deleted (Consolidated)
- `loops-and-iteration.lisp` (merged into control-flow.lisp)
- `pattern-matching.lisp` (merged into control-flow.lisp)
- `mutual-recursion.lisp` (merged into closures.lisp)
- `mutable-storage.lisp` (merged into types.lisp)
- `atoms.lisp` (merged into types.lisp)
- `type-checking.lisp` (merged into types.lisp)
- `keywords.lisp` (renamed to atoms.lisp, then merged into types.lisp)
- `file-io.lisp` (renamed to io.lisp)
- `list-operations.lisp` (merged into lists-and-arrays.lisp)
- `array-operations.lisp` (merged into lists-and-arrays.lisp)
- `math-operations.lisp` (merged into math-and-logic.lisp)
- `logic-operations.lisp` (merged into math-and-logic.lisp)
- `file-modules.lisp` (merged into modules.lisp)
- `recursion.lisp` (merged into closures.lisp)
- `binding.lisp` (merged into scope-and-binding.lisp)
- `scope.lisp` (merged into scope-and-binding.lisp)
- `quasiquote-unquote.lisp` (merged into meta-programming.lisp)
- `macros.lisp` (renamed to meta-programming.lisp)
- `scope-explained.lisp` (merged into scope.lisp)
- `scope-management.lisp` (merged into scope.lisp)
- `math.lisp` (merged into math-operations.lisp)
- `arithmetic.lisp` (merged into math-operations.lisp)
- `arithmetic-predicates.lisp` (merged into math-operations.lisp)
- `universal-length.lisp` (content redistributed to related files)
- `coroutines-advanced.lisp` (merged into coroutines.lisp)
- `type-conversion.lisp` (merged into type-checking.lisp, then into types.lisp)
- `finally-clause.lisp` (merged into exceptions.lisp)

### Polymorphic `length` Function Notes
The `length` function is polymorphic and works on all sequence types:
- **Lists**: See `lists-and-arrays.lisp`
- **Strings**: See `string-operations.lisp`
- **Arrays**: See `lists-and-arrays.lisp`
- **Tables/Structs**: See `tables-and-structs.lisp`

Each file now includes a note about the polymorphic nature of `length`.

### Earlier Consolidations
- `fibonacci.lisp` + `recursive-define.lisp` + `mutual-recursion.lisp` → **recursion.lisp**
- `let-star-binding.lisp` → **binding.lisp**
- `hello.lisp` + `hello-shebang.lisp` → **hello.lisp**
- `macros.lisp` + `meta-programming.lisp` → **macros.lisp**
- `list.lisp` → **list-operations.lisp**
- `concurrency-spawn-join.lisp` → **concurrency.lisp**

### Shared Assertions Library
- Created **assertions.lisp** as reference documentation
- All examples include inline assertion definitions for portability
- Standard assertion functions: assert-eq, assert-true, assert-false, assert-list-eq

All examples serve as executable documentation and contracts for the Elle Lisp
implementation.
