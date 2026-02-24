# Elle Builtins Reference

This document provides comprehensive documentation for all built-in primitives in Elle, including their semantics, argument requirements, and usage examples with output.

## Table of Contents

1. [Arithmetic Operations](#arithmetic-operations)
2. [Comparison Operations](#comparison-operations)
3. [Logic Operations](#logic-operations)
4. [List Operations](#list-operations)
5. [String Operations](#string-operations)
6. [Type Operations](#type-operations)
7. [Math Functions](#math-functions)
8. [Array Operations](#array-operations)
9. [Table Operations](#table-operations)
10. [Struct Operations](#struct-operations)
11. [Higher-Order Functions](#higher-order-functions)
12. [Exception Handling](#exception-handling)
13. [Condition System](#condition-system)
14. [File I/O](#file-io)
15. [Concurrency](#concurrency)
16. [Debugging & Meta](#debugging--meta)
17. [FFI Operations](#ffi-operations)

---

## Arithmetic Operations

### `+` (Addition)

**Semantics**: Adds all numeric arguments together.

**Usage**:
```lisp
(+ 1 2 3)
⟹ 6

(+ 10 20)
⟹ 30

(+)
⟹ 0

(+ 5)
⟹ 5

(+ 1.5 2.5)
⟹ 4.0
```

### `-` (Subtraction)

**Semantics**: Subtracts subsequent arguments from the first argument. With a single argument, negates it.

**Usage**:
```lisp
(- 10 3)
⟹ 7

(- 10 3 2)
⟹ 5

(- 5)
⟹ -5

(- 100 25 10)
⟹ 65
```

### `*` (Multiplication)

**Semantics**: Multiplies all numeric arguments together.

**Usage**:
```lisp
(* 2 3 4)
⟹ 24

(* 5 10)
⟹ 50

(*)
⟹ 1

(* 3.5 2)
⟹ 7.0
```

### `/` (Division)

**Semantics**: Divides the first argument by subsequent arguments sequentially.

**Usage**:
```lisp
(/ 20 4)
⟹ 5

(/ 100 2 5)
⟹ 10

(/ 15 3)
⟹ 5

(/ 7 2)
⟹ 3.5
```

### `mod` / `%` (Modulo)

**Semantics**: Returns remainder after integer division. `%` is an alias for `mod`.

**Usage**:
```lisp
(mod 10 3)
⟹ 1

(% 17 5)
⟹ 2

(mod 20 4)
⟹ 0

(% 7 3)
⟹ 1
```

### `rem` (Remainder)

**Semantics**: Returns remainder of division (similar to mod, with different behavior for negative numbers).

**Usage**:
```lisp
(rem 10 3)
⟹ 1

(rem 20 6)
⟹ 2
```

### `abs` (Absolute Value)

**Semantics**: Returns the absolute value of a number.

**Usage**:
```lisp
(abs -5)
⟹ 5

(abs 10)
⟹ 10

(abs -3.14)
⟹ 3.14
```

### `min` (Minimum)

**Semantics**: Returns the minimum of all arguments.

**Usage**:
```lisp
(min 5 2 8)
⟹ 2

(min 10)
⟹ 10

(min 3.5 2.1 4.0)
⟹ 2.1
```

### `max` (Maximum)

**Semantics**: Returns the maximum of all arguments.

**Usage**:
```lisp
(max 5 2 8)
⟹ 8

(max 10)
⟹ 10

(max 3.5 2.1 4.0)
⟹ 4.0
```

### `even?` (Even Predicate)

**Semantics**: Returns `#t` if the number is even, `#f` otherwise.

**Usage**:
```lisp
(even? 4)
⟹ #t

(even? 7)
⟹ #f

(even? 0)
⟹ #t
```

### `odd?` (Odd Predicate)

**Semantics**: Returns `#t` if the number is odd, `#f` otherwise.

**Usage**:
```lisp
(odd? 5)
⟹ #t

(odd? 4)
⟹ #f

(odd? 1)
⟹ #t
```

---

## Comparison Operations

### `=` (Equality)

**Semantics**: Tests if all arguments are numerically equal.

**Usage**:
```lisp
(= 5 5)
⟹ #t

(= 5 5 5)
⟹ #t

(= 5 6)
⟹ #f

(= 10 10.0)
⟹ #t
```

### `<` (Less Than)

**Semantics**: Tests if first argument is less than second. Requires exactly 2 arguments.

**Usage**:
```lisp
(< 1 2)
⟹ #t

(< 5 5)
⟹ #f

(< 10 5)
⟹ #f

(< 1 3)
⟹ #t
```

### `>` (Greater Than)

**Semantics**: Tests if first argument is greater than second. Requires exactly 2 arguments.

**Usage**:
```lisp
(> 3 2)
⟹ #t

(> 5 5)
⟹ #f

(> 10 20)
⟹ #f

(> 20 10)
⟹ #t
```

### `<=` (Less Than or Equal)

**Semantics**: Tests if first argument is less than or equal to second. Requires exactly 2 arguments.

**Usage**:
```lisp
(<= 1 2)
⟹ #t

(<= 5 5)
⟹ #t

(<= 10 5)
⟹ #f

(<= 5 5)
⟹ #t
```

### `>=` (Greater Than or Equal)

**Semantics**: Tests if first argument is greater than or equal to second. Requires exactly 2 arguments.

**Usage**:
```lisp
(>= 3 2)
⟹ #t

(>= 5 5)
⟹ #t

(>= 1 5)
⟹ #f

(>= 10 5)
⟹ #t
```

---

## Logic Operations

### `not` (Logical Not)

**Semantics**: Returns `#t` if argument is falsy, `#f` if truthy.

**Usage**:
```lisp
(not #f)
⟹ #t

(not #t)
⟹ #f

(not 0)
⟹ #f

(not nil)
⟹ #t
```

### `and` (Logical And)

**Semantics**: Returns first falsy value, or last value if all are truthy.

**Usage**:
```lisp
(and #t #t #t)
⟹ #t

(and #t #f #t)
⟹ #f

(and 1 2 3)
⟹ 3

(and #f 2 3)
⟹ #f

(and)
⟹ #t
```

### `or` (Logical Or)

**Semantics**: Returns first truthy value, or last value if all are falsy.

**Usage**:
```lisp
(or #f #f #t)
⟹ #t

(or #f 2 3)
⟹ 2

(or #f #f #f)
⟹ #f

(or)
⟹ #f
```

### `xor` (Logical Xor)

**Semantics**: Returns `#t` if odd number of arguments are truthy, `#f` otherwise.

**Usage**:
```lisp
(xor #t #f)
⟹ #t

(xor #t #t)
⟹ #f

(xor #t #f #t)
⟹ #t

(xor #f #f)
⟹ #f
```

---

## List Operations

### `cons` (Construct)

**Semantics**: Creates a list cell with head and tail.

**Usage**:
```lisp
(cons 1 (list 2 3))
⟹ (1 2 3)

(cons 'a '(b c))
⟹ (a b c)

(cons 1 nil)
⟹ (1)

(cons 10 20)
⟹ (10 . 20)
```

### `first` (Head)

**Semantics**: Returns the first element of a list.

**Usage**:
```lisp
(first (list 1 2 3))
⟹ 1

(first '(a b c))
⟹ a

(first (cons 'x '(y z)))
⟹ x
```

### `rest` (Tail)

**Semantics**: Returns all elements except the first.

**Usage**:
```lisp
(rest (list 1 2 3))
⟹ (2 3)

(rest '(a))
⟹ ()

(rest (cons 1 (list 2 3)))
⟹ (2 3)
```

### `list` (Create List)

**Semantics**: Creates a list from arguments.

**Usage**:
```lisp
(list 1 2 3)
⟹ (1 2 3)

(list)
⟹ ()

(list 'a 'b 'c)
⟹ (a b c)

(list 1 "hello" #t)
⟹ (1 "hello" #t)
```

### `length` (List Length)

**Semantics**: Returns the number of elements in a list.

**Usage**:
```lisp
(length (list 1 2 3))
⟹ 3

(length '(a b c d e))
⟹ 5

(length (list))
⟹ 0
```

### `append` (Concatenate)

**Semantics**: Concatenates multiple lists.

**Usage**:
```lisp
(append (list 1 2) (list 3 4))
⟹ (1 2 3 4)

(append '(a) '(b) '(c))
⟹ (a b c)

(append (list) (list 1 2))
⟹ (1 2)
```

### `reverse` (Reverse List)

**Semantics**: Reverses the order of elements in a list.

**Usage**:
```lisp
(reverse (list 1 2 3))
⟹ (3 2 1)

(reverse '(a b c d))
⟹ (d c b a)

(reverse (list))
⟹ ()
```

### `nth` (Index Access)

**Semantics**: Returns the nth element (0-indexed).

**Usage**:
```lisp
(nth 0 (list 'a 'b 'c))
⟹ a

(nth 2 '(10 20 30))
⟹ 30

(nth 1 (list 1 2 3))
⟹ 2
```

### `last` (Last Element)

**Semantics**: Returns the last element of a list.

**Usage**:
```lisp
(last (list 1 2 3))
⟹ 3

(last '(a b c))
⟹ c

(last (list 'x))
⟹ x
```

### `take` (Take First N)

**Semantics**: Returns first n elements.

**Usage**:
```lisp
(take 2 (list 1 2 3 4 5))
⟹ (1 2)

(take 3 '(a b c d))
⟹ (a b c)

(take 0 (list 1 2 3))
⟹ ()
```

### `drop` (Drop First N)

**Semantics**: Returns all elements except first n.

**Usage**:
```lisp
(drop 2 (list 1 2 3 4 5))
⟹ (3 4 5)

(drop 1 '(a b c))
⟹ (b c)

(drop 0 (list 1 2 3))
⟹ (1 2 3)
```

---

## String Operations

### `string-append` (Concatenate Strings)

**Semantics**: Concatenates multiple strings.

**Usage**:
```lisp
(string-append "hello" " " "world")
⟹ "hello world"

(string-append "foo" "bar")
⟹ "foobar"

(string-append)
⟹ ""
```

### `string-upcase` (Uppercase)

**Semantics**: Converts string to uppercase.

**Usage**:
```lisp
(string-upcase "hello")
⟹ "HELLO"

(string-upcase "Hello World")
⟹ "HELLO WORLD"

(string-upcase "123")
⟹ "123"
```

### `string-downcase` (Lowercase)

**Semantics**: Converts string to lowercase.

**Usage**:
```lisp
(string-downcase "HELLO")
⟹ "hello"

(string-downcase "Hello World")
⟹ "hello world"

(string-downcase "ABC123")
⟹ "abc123"
```

### `substring` (Extract Substring)

**Semantics**: Extracts substring from start (inclusive) to end (exclusive) index.

**Usage**:
```lisp
(substring "hello" 1 4)
⟹ "ell"

(substring "abcdef" 0 3)
⟹ "abc"

(substring "test" 2 2)
⟹ ""
```

### `string-index` (Find Character Index)

**Semantics**: Returns index of first occurrence of a single character, or `nil` if not found.

**Usage**:
```lisp
(string-index "hello" "l")
⟹ 2

(string-index "hello" "e")
⟹ 1

(string-index "hello" "x")
⟹ nil
```

### `char-at` (Character at Index)

**Semantics**: Returns character at given index as single-character string.

**Usage**:
```lisp
(char-at "hello" 0)
⟹ "h"

(char-at "world" 4)
⟹ "d"

(char-at "test" 2)
⟹ "s"
```

### `string-split` (Split String)

**Semantics**: Splits a string on a delimiter string (not just single character). Returns a list of strings.

**Usage**:
```lisp
(string-split "a,b,c" ",")
⟹ ("a" "b" "c")

(string-split "hello" "ll")
⟹ ("he" "o")

(string-split "aaa" "a")
⟹ ("" "" "" "")

(string-split "hello" "xyz")
⟹ ("hello")
```

### `string-replace` (Replace Substring)

**Semantics**: Replaces all occurrences of `old` with `new` in a string.

**Usage**:
```lisp
(string-replace "hello world" "world" "elle")
⟹ "hello elle"

(string-replace "aaa" "a" "bb")
⟹ "bbbbbb"

(string-replace "test" "x" "y")
⟹ "test"
```

### `string-trim` (Trim Whitespace)

**Semantics**: Removes leading and trailing whitespace from a string.

**Usage**:
```lisp
(string-trim "  hello  ")
⟹ "hello"

(string-trim "hello")
⟹ "hello"

(string-trim "  ")
⟹ ""
```

### `string-contains?` (Contains Substring)

**Semantics**: Returns `#t` if the first string contains the second string as a substring, `#f` otherwise.

**Usage**:
```lisp
(string-contains? "hello world" "world")
⟹ #t

(string-contains? "hello" "xyz")
⟹ #f

(string-contains? "hello" "")
⟹ #t
```

### `string-starts-with?` (Starts With Prefix)

**Semantics**: Returns `#t` if the string starts with the given prefix, `#f` otherwise.

**Usage**:
```lisp
(string-starts-with? "hello" "hel")
⟹ #t

(string-starts-with? "hello" "world")
⟹ #f

(string-starts-with? "test" "")
⟹ #t
```

### `string-ends-with?` (Ends With Suffix)

**Semantics**: Returns `#t` if the string ends with the given suffix, `#f` otherwise.

**Usage**:
```lisp
(string-ends-with? "hello" "llo")
⟹ #t

(string-ends-with? "hello" "world")
⟹ #f

(string-ends-with? "test" "")
⟹ #t
```

### `string-join` (Join Strings)

**Semantics**: Joins a list of strings with a separator string.

**Usage**:
```lisp
(string-join (list "a" "b" "c") ",")
⟹ "a,b,c"

(string-join (list "hello") " ")
⟹ "hello"

(string-join (list) ",")
⟹ ""
```

### `number->string` (Number to String)

**Semantics**: Converts an integer or float to its string representation.

**Usage**:
```lisp
(number->string 42)
⟹ "42"

(number->string 3.14)
⟹ "3.14"

(number->string -100)
⟹ "-100"
```

---

## Type Operations

### `type-of` (Get Type)

**Semantics**: Returns keyword name of value's type.

**Usage**:
```lisp
(type-of 5)
⟹ :integer

(type-of 3.14)
⟹ :float

(type-of "hello")
⟹ :string

(type-of (list 1 2))
⟹ :list

(type-of nil)
⟹ :nil
```

### `nil?` (Is Nil)

**Semantics**: Returns `#t` if value is nil.

**Usage**:
```lisp
(nil? nil)
⟹ #t

(nil? (list))
⟹ #f

(nil? 0)
⟹ #f

(nil? #f)
⟹ #f
```

### `number?` (Is Number)

**Semantics**: Returns `#t` if value is a number (int or float).

**Usage**:
```lisp
(number? 5)
⟹ #t

(number? 3.14)
⟹ #t

(number? "5")
⟹ #f

(number? #t)
⟹ #f
```

### `string?` (Is String)

**Semantics**: Returns `#t` if value is a string.

**Usage**:
```lisp
(string? "hello")
⟹ #t

(string? 42)
⟹ #f

(string? 'symbol)
⟹ #f
```

### `symbol?` (Is Symbol)

**Semantics**: Returns `#t` if value is a symbol.

**Usage**:
```lisp
(symbol? 'x)
⟹ #t

(symbol? "x")
⟹ #f

(symbol? 42)
⟹ #f
```

### `pair?` (Is List/Pair)

**Semantics**: Returns `#t` if value is a list or cons cell.

**Usage**:
```lisp
(pair? (list 1 2))
⟹ #t

(pair? (cons 1 2))
⟹ #t

(pair? nil)
⟹ #f

(pair? 5)
⟹ #f
```

---

## Math Functions

### `sqrt` (Square Root)

**Semantics**: Returns square root of a number.

**Usage**:
```lisp
(sqrt 4)
⟹ 2.0

(sqrt 16)
⟹ 4.0

(sqrt 2)
⟹ 1.4142135623730951

(sqrt 0)
⟹ 0.0
```

### `pow` (Power)

**Semantics**: Raises first argument to power of second argument.

**Usage**:
```lisp
(pow 2 3)
⟹ 8.0

(pow 10 2)
⟹ 100.0

(pow 2 0)
⟹ 1.0

(pow 5 0.5)
⟹ 2.23606797749979
```

### `floor` (Floor)

**Semantics**: Rounds down to nearest integer.

**Usage**:
```lisp
(floor 3.7)
⟹ 3.0

(floor 5.2)
⟹ 5.0

(floor -2.3)
⟹ -3.0
```

### `ceil` (Ceiling)

**Semantics**: Rounds up to nearest integer.

**Usage**:
```lisp
(ceil 3.2)
⟹ 4.0

(ceil 5.0)
⟹ 5.0

(ceil -2.3)
⟹ -2.0
```

### `round` (Round)

**Semantics**: Rounds to nearest integer.

**Usage**:
```lisp
(round 3.5)
⟹ 4.0

(round 3.4)
⟹ 3.0

(round 2.5)
⟹ 2.0
```

### `sin` (Sine)

**Semantics**: Returns sine of angle in radians.

**Usage**:
```lisp
(sin 0)
⟹ 0.0

(sin (/ pi 2))
⟹ 1.0

(sin pi)
⟹ 1.2246467991473532e-16
```

### `cos` (Cosine)

**Semantics**: Returns cosine of angle in radians.

**Usage**:
```lisp
(cos 0)
⟹ 1.0

(cos (/ pi 2))
⟹ 6.123233995736766e-17

(cos pi)
⟹ -1.0
```

### `tan` (Tangent)

**Semantics**: Returns tangent of angle in radians.

**Usage**:
```lisp
(tan 0)
⟹ 0.0

(tan (/ pi 4))
⟹ 0.9999999999999999
```

### `log` (Natural Logarithm)

**Semantics**: Returns natural logarithm of a number.

**Usage**:
```lisp
(log 1)
⟹ 0.0

(log e)
⟹ 1.0

(log 10)
⟹ 2.302585092994046
```

### `exp` (Exponential)

**Semantics**: Returns e raised to the power of argument.

**Usage**:
```lisp
(exp 0)
⟹ 1.0

(exp 1)
⟹ 2.718281828459045

(exp 2)
⟹ 7.38905609893065
```

### `pi` (Pi Constant)

**Semantics**: Returns the value of π (pi).

**Usage**:
```lisp
pi
⟹ 3.141592653589793

(* pi 2)
⟹ 6.283185307179586
```

### `e` (Euler's Number)

**Semantics**: Returns the value of e (Euler's number).

**Usage**:
```lisp
e
⟹ 2.718281828459045

(pow e 2)
⟹ 7.38905609893065
```

---

## Array Operations

### `array` (Create Array)

**Semantics**: Creates a mutable array from arguments.

**Usage**:
```lisp
(var v (array 1 2 3 4 5))
(length v)
⟹ 5
```

**Note**: Use `length` (polymorphic) instead of `array-length` for getting array length.

### `array-ref` (Array Reference)

**Semantics**: Returns element at given index.

**Usage**:
```lisp
(var v (array 'a 'b 'c))
(array-ref v 1)
⟹ b
```

### `array-set!` (Array Set)

**Semantics**: Sets element at given index (mutates array).

**Usage**:
```lisp
(var v (array 1 2 3))
(array-set! v 1 99)
(array-ref v 1)
⟹ 99
```

---

## Table Operations

Tables are mutable hash maps. Keys and values can be any type.

### `table` (Create Table)

**Semantics**: Creates an empty mutable table or table with initial key-value pairs.

**Usage**:
```lisp
(var t (table))
(table-length t)
⟹ 0

(var t2 (table "a" 1 "b" 2))
(table-length t2)
⟹ 2
```

### `get` (Get Value)

**Semantics**: Gets value for key, returns default if not found.

**Usage**:
```lisp
(var t (table 1 "one" 2 "two"))
(get t 1)
⟹ "one"

(get t 3 "not found")
⟹ "not found"
```

### `put` (Set Value)

**Semantics**: Sets value for key (mutates table).

**Usage**:
```lisp
(var t (table))
(put t "name" "Alice")
(get t "name")
⟹ "Alice"
```

### `del` (Delete Key)

**Semantics**: Removes key-value pair from table.

**Usage**:
```lisp
(var t (table 1 "a" 2 "b"))
(del t 1)
(has-key? t 1)
⟹ #f
```

### `has-key?` (Has Key)

**Semantics**: Returns `#t` if key exists in table.

**Usage**:
```lisp
(var t (table "x" 10))
(has-key? t "x")
⟹ #t

(has-key? t "y")
⟹ #f
```

### `keys` (Get All Keys)

**Semantics**: Returns list of all keys in table.

**Usage**:
```lisp
(var t (table "a" 1 "b" 2))
(keys t)
⟹ ("a" "b")
```

### `values` (Get All Values)

**Semantics**: Returns list of all values in table.

**Usage**:
```lisp
(var t (table "a" 1 "b" 2))
(values t)
⟹ (1 2)
```

---

## Struct Operations

Structs are immutable hash maps. Similar to tables but cannot be modified.

### `struct` (Create Struct)

**Semantics**: Creates immutable struct from key-value pairs.

**Usage**:
```lisp
(var s (struct "name" "Bob" "age" 30))
(struct-length s)
⟹ 2
```

### `struct-get` (Get Value)

**Semantics**: Gets value for key with optional default.

**Usage**:
```lisp
(var s (struct "x" 10 "y" 20))
(struct-get s "x")
⟹ 10

(struct-get s "z" -1)
⟹ -1
```

### `struct-put` (Create New Struct)

**Semantics**: Creates new struct with key-value added/updated.

**Usage**:
```lisp
(var s (struct "a" 1))
(var s2 (struct-put s "b" 2))
(struct-length s)
⟹ 1

(struct-length s2)
⟹ 2
```

### `struct-del` (Create New Struct Without Key)

**Semantics**: Creates new struct with key removed.

**Usage**:
```lisp
(var s (struct "a" 1 "b" 2))
(var s2 (struct-del s "a"))
(struct-has? s2 "a")
⟹ #f
```

### `struct-has?` (Has Key)

**Semantics**: Returns `#t` if key exists.

**Usage**:
```lisp
(var s (struct "name" "Alice"))
(struct-has? s "name")
⟹ #t

(struct-has? s "age")
⟹ #f
```

### `struct-keys` (Get All Keys)

**Semantics**: Returns list of all keys.

**Usage**:
```lisp
(var s (struct "x" 1 "y" 2))
(struct-keys s)
⟹ ("x" "y")
```

### `struct-values` (Get All Values)

**Semantics**: Returns list of all values.

**Usage**:
```lisp
(var s (struct "x" 1 "y" 2))
(struct-values s)
⟹ (1 2)
```

---

## Higher-Order Functions

### `map` (Apply Function to List)

**Semantics**: Applies function to each element, returns list of results.

**Note**: This requires working higher-order functions. Currently has limitations with closures.

**Usage**:
```lisp
(var double (lambda (x) (* x 2)))
(map double (list 1 2 3))
⟹ (2 4 6)

(map (lambda (x) (+ x 1)) '(1 2 3))
⟹ (2 3 4)
```

### `filter` (Select Elements)

**Semantics**: Applies predicate to each element, returns list of elements where predicate is true.

**Usage**:
```lisp
(var is-even (lambda (x) (= (mod x 2) 0)))
(filter is-even (list 1 2 3 4 5))
⟹ (2 4)

(filter (lambda (x) (> x 5)) '(3 7 2 8 1))
⟹ (7 8)
```

### `fold` (Reduce List)

**Semantics**: Applies function to accumulator and each element in sequence.

**Usage**:
```lisp
(var add (lambda (a b) (+ a b)))
(fold add 0 (list 1 2 3 4))
⟹ 10

(fold (lambda (acc x) (+ acc x)) 0 '(10 20 30))
⟹ 60
```

---

## Type Conversions

### `int` (Convert to Integer)

**Semantics**: Converts value to integer.

**Usage**:
```lisp
(int 3.7)
⟹ 3

(int "42")
⟹ 42

(int 5)
⟹ 5
```

### `float` (Convert to Float)

**Semantics**: Converts value to floating-point number.

**Usage**:
```lisp
(float 5)
⟹ 5.0

(float "3.14")
⟹ 3.14

(float 10)
⟹ 10.0
```

### `string` (Convert to String)

**Semantics**: Converts value to string representation.

**Usage**:
```lisp
(string 42)
⟹ "42"

(string 3.14)
⟹ "3.14"

(string 'symbol)
⟹ "symbol"
```

---

## Exception Handling

### `exception` (Create Exception)

**Semantics**: Creates an exception object with message and optional data.

**Usage**:
```lisp
(var e (exception "Error message"))
(exception-message e)
⟹ "Error message"

(var e2 (exception "Invalid input" 42))
(exception-data e2)
⟹ 42
```

### `exception-message` (Get Message)

**Semantics**: Extracts message from exception.

**Usage**:
```lisp
(var e (exception "Something went wrong"))
(exception-message e)
⟹ "Something went wrong"
```

### `exception-data` (Get Data)

**Semantics**: Extracts data payload from exception.

**Usage**:
```lisp
(var e (exception "Error" (list "details" "here")))
(exception-data e)
⟹ ("details" "here")
```

### `throw` (Throw Exception)

**Semantics**: Throws exception, stops execution.

**Usage**:
```lisp
(try
  (throw (exception "Test error"))
  (catch e (string-append "Caught: " (exception-message e))))
⟹ "Caught: Test error"
```

---

## File I/O

### `slurp` (Read File)

**Semantics**: Reads entire file contents as string.

**Usage**:
```lisp
(spit "test.txt" "Hello, World!")
(slurp "test.txt")
⟹ "Hello, World!"
```

### `spit` (Write File)

**Semantics**: Writes string to file, creates or overwrites.

**Usage**:
```lisp
(spit "output.txt" "File content")
(file-exists? "output.txt")
⟹ #t
```

### `append-file` (Append to File)

**Semantics**: Appends string to end of file.

**Usage**:
```lisp
(spit "log.txt" "Line 1\n")
(append-file "log.txt" "Line 2\n")
```

### `file-exists?` (Check File Exists)

**Semantics**: Returns `#t` if file exists.

**Usage**:
```lisp
(spit "temp.txt" "data")
(file-exists? "temp.txt")
⟹ #t

(file-exists? "nonexistent.txt")
⟹ #f
```

### `file?` (Is File)

**Semantics**: Returns `#t` if path is a regular file.

**Usage**:
```lisp
(write-file "test.txt" "content")
(file? "test.txt")
⟹ #t

(file? ".")
⟹ #f
```

### `directory?` (Is Directory)

**Semantics**: Returns `#t` if path is a directory.

**Usage**:
```lisp
(directory? ".")
⟹ #t

(directory? "/nonexistent")
⟹ #f
```

### `delete-file` (Delete File)

**Semantics**: Deletes a file.

**Usage**:
```lisp
(write-file "delete_me.txt" "temp")
(delete-file "delete_me.txt")
(file-exists? "delete_me.txt")
⟹ #f
```

### `create-directory` (Create Directory)

**Semantics**: Creates a single directory.

**Usage**:
```lisp
(create-directory "newdir")
(directory? "newdir")
⟹ #t
```

### `create-directory-all` (Create Directory Tree)

**Semantics**: Creates directory and all parent directories.

**Usage**:
```lisp
(create-directory-all "a/b/c")
(directory? "a/b/c")
⟹ #t
```

### `delete-directory` (Delete Directory)

**Semantics**: Deletes empty directory.

**Usage**:
```lisp
(create-directory "toremove")
(delete-directory "toremove")
(directory? "toremove")
⟹ #f
```

### `file-size` (Get File Size)

**Semantics**: Returns file size in bytes.

**Usage**:
```lisp
(write-file "test.txt" "12345")
(file-size "test.txt")
⟹ 5
```

### `list-directory` (List Directory Contents)

**Semantics**: Returns list of filenames in directory.

**Usage**:
```lisp
(create-directory "mydir")
(write-file "mydir/file1.txt" "a")
(write-file "mydir/file2.txt" "b")
(list-directory "mydir")
⟹ ("file1.txt" "file2.txt")
```

### `absolute-path` (Get Absolute Path)

**Semantics**: Converts relative path to absolute.

**Usage**:
```lisp
(absolute-path ".")
⟹ "/home/user/current/directory"

(absolute-path "file.txt")
⟹ "/home/user/file.txt"
```

### `current-directory` (Get Current Directory)

**Semantics**: Returns current working directory path.

**Usage**:
```lisp
(current-directory)
⟹ "/home/user/documents"
```

### `change-directory` (Change Directory)

**Semantics**: Changes current working directory.

**Usage**:
```lisp
(change-directory "/tmp")
(current-directory)
⟹ "/tmp"
```

### `file-name` (Get Filename)

**Semantics**: Extracts filename from path.

**Usage**:
```lisp
(file-name "/home/user/document.txt")
⟹ "document.txt"

(file-name "folder/subfolder/file.lisp")
⟹ "file.lisp"
```

### `file-extension` (Get Extension)

**Semantics**: Extracts file extension.

**Usage**:
```lisp
(file-extension "document.txt")
⟹ "txt"

(file-extension "archive.tar.gz")
⟹ "gz"

(file-extension "noextension")
⟹ ""
```

### `parent-directory` (Get Parent Path)

**Semantics**: Returns parent directory path.

**Usage**:
```lisp
(parent-directory "/home/user/documents")
⟹ "/home/user"

(parent-directory "folder/file.txt")
⟹ "folder"
```

### `join-path` (Join Path Components)

**Semantics**: Joins path components into single path.

**Usage**:
```lisp
(join-path "home" "user" "documents")
⟹ "home/user/documents"

(join-path "/usr" "local" "bin")
⟹ "/usr/local/bin"
```

### `rename-file` (Rename File)

**Semantics**: Renames file.

**Usage**:
```lisp
(write-file "old.txt" "content")
(rename-file "old.txt" "new.txt")
(file-exists? "new.txt")
⟹ #t
```

### `copy-file` (Copy File)

**Semantics**: Copies file.

**Usage**:
```lisp
(write-file "original.txt" "data")
(copy-file "original.txt" "copy.txt")
(file-exists? "copy.txt")
⟹ #t
```

### `read-lines` (Read File Lines)

**Semantics**: Reads file as list of lines.

**Usage**:
```lisp
(write-file "test.txt" "Line 1\nLine 2\nLine 3")
(read-lines "test.txt")
⟹ ("Line 1" "Line 2" "Line 3")
```

---

## Concurrency

### `spawn` (Spawn Thread)

**Semantics**: Creates new thread that executes a closure. The closure must capture only immutable values.

**Usage**:
```lisp
(var t (spawn (fn () (display "Running in thread"))))
```

### `join` (Wait for Thread)

**Semantics**: Waits for thread to complete and returns its result.

**Usage**:
```lisp
(var t (spawn (fn () (+ 2 2))))
(join t)
⟹ 4
```

### `current-thread-id` (Get Thread ID)

**Semantics**: Returns identifier of current thread as a string.

**Usage**:
```lisp
(current-thread-id)
⟹ "ThreadId(1)"
```

---

## Clock & Time

### `clock/monotonic` (Monotonic Clock)

**Semantics**: Returns seconds elapsed since process start as a float. Uses a monotonic clock that never goes backwards. Suitable for measuring durations.

**Usage**:
```lisp
(clock/monotonic)
⟹ 0.001234

(let ((t1 (clock/monotonic)))
  (time/sleep 0.1)
  (- (clock/monotonic) t1))
⟹ 0.100... ; approximately 0.1 seconds
```

### `clock/realtime` (Wall Clock)

**Semantics**: Returns seconds since Unix epoch (January 1, 1970) as a float. Subject to NTP adjustments — not suitable for measuring durations.

**Usage**:
```lisp
(clock/realtime)
⟹ 1740268800.123456
```

### `time/sleep` (Sleep)

**Semantics**: Pauses execution for the given number of seconds. Accepts integers or floats. Duration must be finite and non-negative.

**Usage**:
```lisp
(time/sleep 1)       ; sleep 1 second
(time/sleep 0.5)     ; sleep 500 milliseconds
(time/sleep 0.001)   ; sleep 1 millisecond
```

### `time/elapsed` (Measure Elapsed Time)

**Semantics**: Calls a zero-argument function and returns a list of `(result elapsed-seconds)`. Defined in Elle using `clock/monotonic`.

**Usage**:
```lisp
(time/elapsed (fn () (+ 1 2)))
⟹ (3 0.000012)

(let ((result (time/elapsed (fn () (time/sleep 0.1) 42))))
  (first result))
⟹ 42
```

### `time/stopwatch` (Create Stopwatch)

**Semantics**: Returns a coroutine that yields elapsed seconds since creation each time it is resumed. Defined in Elle using `clock/monotonic` and `coro/new`.

**Usage**:
```lisp
(var sw (time/stopwatch))
(coro/resume sw)
⟹ 0.000001

(time/sleep 0.1)
(coro/resume sw)
⟹ 0.100...
```

---

## Debugging & Meta

### `display` (Print Value)

**Semantics**: Prints value to output without newline.

**Usage**:
```lisp
(display "Hello")
(display 42)
(display (list 1 2 3))
; Output: Hello42(1 2 3)
```

### `newline` (Print Newline)

**Semantics**: Prints newline character.

**Usage**:
```lisp
(display "Line 1")
(newline)
(display "Line 2")
(newline)
; Output:
; Line 1
; Line 2
```

### `debug-print` (Debug Output)

**Semantics**: Prints value with debug information.

**Usage**:
```lisp
(debug-print "value" 42)
; Output: value: 42 (type: int)
```

### `trace` (Trace Value)

**Semantics**: Prints a labeled value to stderr and returns the value. First argument is a label (string or symbol), second is the value to trace.

**Usage**:
```lisp
(trace "x" 42)
; Prints: [TRACE] x: 42
⟹ 42

(trace "result" (+ 1 2))
; Prints: [TRACE] result: 3
⟹ 3
```

### `memory-usage` (Get Memory Usage)

**Semantics**: Returns current memory usage in bytes.

**Usage**:
```lisp
(memory-usage)
⟹ 1048576
```

### `gensym` (Generate Symbol)

**Semantics**: Generates unique symbol with optional prefix.

**Usage**:
```lisp
(gensym)
⟹ #:G1

(gensym "x")
⟹ #:x2

(gensym "var")
⟹ #:var3
```

### `string->keyword` (String to Keyword)

**Semantics**: Converts a string to an interned keyword value.

**Usage**:
```lisp
(string->keyword "foo")
⟹ :foo

(= (string->keyword "bar") (string->keyword "bar"))
⟹ #t
```

### `call-count` (Closure Call Count)

**Semantics**: Returns the number of times a closure has been called. Returns 0 for non-closures.

**Usage**:
```lisp
(let ((f (fn (x) x)))
  (f 1) (f 2) (f 3)
  (call-count f))
⟹ 3
```

### `global?` (Global Binding Check)

**Semantics**: Returns `#t` if a symbol is bound as a global, `#f` otherwise.

**Usage**:
```lisp
(global? '+)
⟹ #t

(global? 'nonexistent)
⟹ #f
```

---

## FFI Operations

### `load-library` (Load C Library)

**Semantics**: Loads C library for FFI use.

**Usage**:
```lisp
(load-library "libc")
; Loads C standard library
```

### `call-c-function` (Call C Function)

**Semantics**: Calls C function from loaded library.

**Usage**:
```lisp
(load-library "libc")
(call-c-function "strlen" (list "hello"))
⟹ 5
```

### `list-libraries` (List Loaded Libraries)

**Semantics**: Returns list of currently loaded libraries.

**Usage**:
```lisp
(list-libraries)
⟹ ("libc" "libm")
```

### Other FFI Primitives

- `load-header-with-lib` - Load C header with library
- `define-enum` - Define C enum mapping
- `make-c-callback` - Create C callback function
- `free-callback` - Free C callback
- `register-allocation` - Register memory allocation
- `memory-stats` - Get memory statistics
- `type-check` - Check FFI type
- `null-pointer?` - Check for null pointer
- `ffi-last-error` - Get last FFI error
- `with-ffi-safety-checks` - Enable FFI safety checks

---

## JSON Operations

JSON parsing and serialization for working with JSON data. All JSON primitives are hand-written without external dependencies.

### `json-parse` (Parse JSON)

**Semantics**: Parses a JSON string into Elle values.

**Type Mapping**:
- JSON `null` → Elle `nil`
- JSON `true` / `false` → Elle `#t` / `#f`
- JSON integer → Elle `Int`
- JSON float → Elle `Float`
- JSON string → Elle `String`
- JSON array → Elle List (cons-based)
- JSON object → Elle Table (mutable map)

**Handles**:
- All JSON escape sequences: `\"`, `\\`, `\/`, `\b`, `\f`, `\n`, `\r`, `\t`, `\uXXXX`
- Nested structures to arbitrary depth
- Whitespace between tokens
- Proper error messages with position information

**Usage**:
```lisp
(json-parse "null")
⟹ nil

(json-parse "true")
⟹ #t

(json-parse "42")
⟹ 42

(json-parse "3.14")
⟹ 3.14

(json-parse "\"hello\"")
⟹ "hello"

(json-parse "[1, 2, 3]")
⟹ (1 2 3)

(json-parse "{\"name\": \"Alice\", \"age\": 30}")
⟹ #<table String("age")=30 String("name")="Alice">

; Access parsed object fields
(var user (json-parse "{\"name\": \"Bob\", \"active\": true}"))
(get user "name")
⟹ "Bob"

(get user "active")
⟹ #t
```

**Errors**:
- Empty input: `"Unexpected end of input: empty JSON"`
- Trailing content: `"Unexpected trailing content at position X"`
- Invalid tokens: `"Unexpected character 'X' at position Y"`
- Unterminated strings: `"Unterminated string"`
- Unclosed arrays/objects: `"Unexpected end of input in array/object"`

### `json-serialize` (Serialize to Compact JSON)

**Semantics**: Serializes an Elle value to compact JSON (no whitespace).

**Type Mapping**:
- Elle `nil` → JSON `null`
- Elle `#t` / `#f` → JSON `true` / `false`
- Elle `Int` → JSON number
- Elle `Float` → JSON number (always includes decimal point)
- Elle `String` → JSON string with proper escaping
- Elle List → JSON array
- Elle Array → JSON array
- Elle Table → JSON object (keys must be strings)
- Elle Struct → JSON object (keys must be strings)

**Usage**:
```lisp
(json-serialize nil)
⟹ "null"

(json-serialize #t)
⟹ "true"

(json-serialize 42)
⟹ "42"

(json-serialize 3.14)
⟹ "3.14"

(json-serialize "hello")
⟹ "\"hello\""

(json-serialize (list 1 2 3))
⟹ "[1,2,3]"

(var config (table))
(put config "name" "MyApp")
(put config "version" "1.0.0")
(json-serialize config)
⟹ "{\"name\":\"MyApp\",\"version\":\"1.0.0\"}"
```

**String Escaping**:
- `"` → `\"`
- `\` → `\\`
- newline → `\n`
- tab → `\t`
- carriage return → `\r`
- backspace → `\b`
- form feed → `\f`
- Control characters → `\uXXXX`

**Errors**:
- Non-string table keys: `"Table keys must be strings for JSON serialization"`
- Closures: `"Cannot serialize closures to JSON"`
- Native functions: `"Cannot serialize native functions to JSON"`
- Symbols: `"Cannot serialize symbols to JSON"`

### `json-serialize-pretty` (Serialize to Pretty JSON)

**Semantics**: Serializes an Elle value to pretty-printed JSON with 2-space indentation.

**Usage**:
```lisp
(var user (table))
(put user "name" "Alice")
(put user "age" 30)
(put user "active" #t)

(json-serialize-pretty user)
⟹ "{
  \"active\": true,
  \"age\": 30,
  \"name\": \"Alice\"
}"

(var users (list user))
(json-serialize-pretty users)
⟹ "[
  {
    \"active\": true,
    \"age\": 30,
    \"name\": \"Alice\"
  }
]"
```

**Format**:
- 2-space indentation per level
- Newlines after opening braces/brackets
- Newlines before closing braces/brackets
- Space after colons in objects
- Comma-separated elements on separate lines

### Round-trip Example

Parse JSON, modify, and serialize back:

```lisp
(var original "{\"product\": \"Widget\", \"price\": 19.99}")
(var parsed (json-parse original))

; Modify the parsed data
(put parsed "price" 24.99)
(put parsed "discount" 0.1)

; Serialize back to JSON
(json-serialize parsed)
⟹ "{\"discount\":0.1,\"price\":24.99,\"product\":\"Widget\"}"

; Pretty print for readability
(json-serialize-pretty parsed)
⟹ "{
  \"discount\": 0.1,
  \"price\": 24.99,
  \"product\": \"Widget\"
}"
```

---

## Process Control

### `halt` (Halt VM)

**Semantics**: Signals the VM to stop execution and return a value to the host. Unlike `exit`, does not terminate the process. The signal is maskable by fiber signal masks but non-resumable: the halted fiber is Dead.

**Usage**:
```lisp
(halt)         ; halt, return nil
(halt value)   ; halt, return value
```

**Examples**:
```lisp
(halt 42)
⟹ 42

(begin (halt 1) 2)
⟹ 1
```

---

## Module & Package Operations

### `import-file` (Import Module)

**Semantics**: Loads and evaluates Elle code from file.

**Usage**:
```lisp
(import-file "utils.lisp")
; Loads definitions from utils.lisp
```

### `add-module-path` (Add Module Search Path)

**Semantics**: Adds directory to module search path.

**Usage**:
```lisp
(add-module-path "lib")
(import-file "mymodule")
; Searches for mymodule in lib/
```

### `package-version` (Get Package Version)

**Semantics**: Returns package version string.

**Usage**:
```lisp
(package-version)
⟹ "0.1.0"
```

### `package-info` (Get Package Info)

**Semantics**: Returns package metadata.

**Usage**:
```lisp
(package-info)
⟹ (name "elle" version "0.1.0" ...)
```

### `expand-macro` (Expand Macro)

**Semantics**: Expands macro to underlying code.

**Usage**:
```lisp
(expand-macro '(when #t (display "yes")))
⟹ (if #t (begin (display "yes")))
```

### `macro?` (Is Macro)

**Semantics**: Returns `#t` if value is a macro.

**Usage**:
```lisp
(macro? when)
⟹ #t

(macro? +)
⟹ #f
```

## Condition System

> **Deprecated.** These primitives will be replaced by the fiber/signal
> model. See `docs/FIBERS.md`. They still work but will be removed.

The condition system provides sophisticated error handling beyond simple exceptions, allowing custom signal types with registered handlers.

### `define-condition` (Define Condition Type)

**Semantics**: Creates a new condition type with named fields and default values.

**Usage**:
```lisp
(var-condition :validation-error
  (message "Validation failed")
  (field "unknown")
  (value nil))

(var-condition :network-error
  (message "Network error")
  (url "")
  (status-code 0)
  (retry #f))
```

### `define-handler` (Register Condition Handler)

**Semantics**: Registers a handler function for a condition type. Multiple handlers can be registered.

**Usage**:
```lisp
(var-handler :validation-error
  (lambda (c)
    (display "Validation Error: ")
    (display (condition-get c 'message))
    (newline)))

(var-handler :validation-error
  (lambda (c)
    (display "  Field: ")
    (display (condition-get c 'field))
    (newline)))
```

### `signal` (Signal Condition)

**Semantics**: Triggers a condition, calling all registered handlers in order.

**Usage**:
```lisp
(signal :validation-error
  :message "Invalid email format"
  :field "email"
  :value "not-an-email")
```

Handlers are invoked:
```
Validation Error: Invalid email format
  Field: email
```

### `condition-get` (Get Condition Field)

**Semantics**: Extracts a field value from a condition object.

**Usage**:
```lisp
(var-handler :validation-error
  (lambda (c)
    (display (condition-get c 'message))
    (display " in field: ")
    (display (condition-get c 'field))))
```

### `catch-condition` (Catch Specific Condition)

**Semantics**: Intercepts a specific condition type during execution.

**Usage**:
```lisp
(catch-condition :validation-error
  (validate-user-input username)
  (lambda (c)
    (display "Fix the error: ")
    (display (condition-get c 'message))
    (newline)))
```

### `condition-catch` (Catch Any Condition)

**Semantics**: Generic handler that catches any condition.

**Usage**:
```lisp
(condition-catch
  (signal :any-error :data "info")
  (lambda (condition-type condition-data)
    (display "Caught ")
    (display condition-type)
    (display ": ")
    (display (condition-get condition-data 'message))))
```

### Example: Input Validation System

```lisp
; Define validation conditions
(var-condition :field-error
  (message "Field validation failed")
  (field "unknown")
  (constraint "unknown"))

; Register multiple handlers
(var-handler :field-error
  (lambda (c)
    (display "ERROR: ")
    (display (condition-get c 'field'))
    (display " - ")
    (display (condition-get c 'message'))
    (newline)))

(var-handler :field-error
  (lambda (c)
    (display "  Required: ")
    (display (condition-get c 'constraint'))
    (newline)))

; Validation function
(def (validate-email email)
  (unless (string-contains? email "@")
    (signal :field-error
      :message "Must contain @"
      :field "email"
      :constraint "valid email format")))

; Use it
(validate-email "bad-email")
```

Output:
```
ERROR: email - Must contain @
  Required: valid email format
```

---

## Notes on Semantics

- **Truthiness**: In Elle, `#f` (false) and `nil` are falsy; all other values including the empty list `()` are truthy.
- **List Semantics**: Lists are represented as cons cells terminated by `()` (the empty list). Note that `nil` and `()` are distinct values:
  - `nil` - Represents absence of value, used for "no result" or undefined. It is **falsy**.
  - `()` - The empty list. It is a valid list value and is **truthy**.
- **Numeric Types**: Operations work with both integers and floats; results may be promoted to float.
- **Error Handling**: Primitives return `Result<Value, String>` errors caught by the VM.
- **Mutability**: Tables and vectors are mutable; structs and lists are immutable.

---

## Known Limitations

- **Higher-Order Functions**: Currying and function factories have issues with environment capture (Issues #77, #78)
- **Macro System**: Limited macro support; gensym for hygiene not fully implemented
- **FFI**: FFI operations require careful type handling; not all C types fully supported

See the main README for more information and examples.
