#!/usr/bin/env elle

# Basics — Elle's type system and value representations
#
# Demonstrates:
#   Immediates       — nil, booleans, integers, floats, symbols, keywords
#   Truthiness       — only nil and false are falsy
#   Arithmetic       — +, -, *, /, mod, abs, min, max, even?, odd?
#   Math             — math/sqrt, math/sin, math/cos, math/pow, math/pi, ...
#   Comparison/logic — =, <, >, not, and, or (short-circuiting)
#   Bitwise          — bit/and, bit/or, bit/xor, bit/not, bit/shl, bit/shr
#   Type conversions — number->string, string->integer, integer, float, ...
#   Mutability split — [tuple] vs @[array], {struct} vs @{table}, "str" vs @"buf"
#   Bytes and blobs  — immutable/mutable binary data
#   Boxes            — first-class mutable cells
#   Equality         — = does structural equality on data, reference on functions

(import-file "./examples/assertions.lisp")


# ========================================
# 1. Immediates
# ========================================

# nil — the absence of a value
(assert-true (nil? nil) "nil? on nil")         # nil is only equal to itself
(assert-false (nil? false) "nil? on false")     # false is NOT nil
(assert-false (nil? 0) "nil? on 0")            # 0 is NOT nil

# Booleans — true and false, that's it
(assert-true (boolean? true) "boolean? on true")
(assert-true (boolean? false) "boolean? on false")
(assert-false (boolean? 1) "boolean? not on int")  # 1 is not a boolean

# Integers are 48-bit signed: -140,737,488,355,328 to 140,737,488,355,327.
# Floats are 64-bit IEEE 754 doubles.
# (Both are NaN-boxed into a single 8-byte Value representation.)
(assert-true (number? 42) "number? on int")     # integers are numbers
(assert-true (number? 3.14) "number? on float") # floats are numbers
(assert-eq (type 42) :integer "42 is integer")  # type returns a keyword
(assert-eq (type 3.14) :float "3.14 is float")
(assert-false (number? "42") "number? not on string")  # "42" is a string

# Symbols and keywords — both are interned (fast equality)
(assert-true (symbol? 'hello) "symbol? on quoted symbol")  # 'x quotes a symbol
(assert-true (keyword? :hello) "keyword? on keyword")      # :x is a keyword
(assert-false (= 'name :name) "symbol != keyword with same name")

# Empty list is its own thing — not nil
(assert-true (empty? (list)) "empty? on empty list")    # use empty? for lists
(assert-false (nil? (list)) "empty list is NOT nil")     # () and nil differ!

# (type x) returns a keyword describing x's type
(display "  (type 42)   = ") (print (type 42))       # :integer
(display "  (type 3.14) = ") (print (type 3.14))     # :float
(display "  (type \"hi\") = ") (print (type "hi"))    # :string
(display "  (type true) = ") (print (type true))     # :boolean
(display "  (type nil)  = ") (print (type nil))      # :nil
(display "  (type :foo) = ") (print (type :foo))     # :keyword
(display "  (type 'foo) = ") (print (type 'foo))     # :symbol
(assert-eq (type 42) :integer "type of int")
(assert-eq (type "hi") :string "type of string")
(assert-eq (type nil) :nil "type of nil")


# ========================================
# 2. Truthiness
# ========================================

# Only nil and false are falsy — everything else is truthy.
# This differs from C/Python/JS where 0, "", [] are falsy.
#
# (if test then else) evaluates test; if truthy, returns then, otherwise else.
# else is optional and defaults to nil. if is an expression — it returns a value.
(assert-false (if nil true false) "nil is falsy")        # nil → else branch
(assert-false (if false true false) "false is falsy")    # false → else branch

(assert-true (if 0 true false) "0 is truthy")           # unlike C/Python
(assert-true (if "" true false) "empty string is truthy")
(assert-true (if (list) true false) "empty list () is truthy")  # unlike nil!
(assert-true (if [] true false) "empty tuple is truthy")
(assert-true (if @[] true false) "empty array is truthy")
(assert-true (if :keyword true false) "keyword is truthy")
(assert-true (if 'symbol true false) "symbol is truthy")

(display "  falsy:  nil=") (display (if nil :T :F))
  (display " false=") (print (if false :T :F))
(display "  truthy: 0=") (display (if 0 :T :F))
  (display " \"\"=") (display (if "" :T :F))
  (display " ()=") (display (if (list) :T :F))
  (display " []=") (print (if [] :T :F))


# ========================================
# 3. Arithmetic
# ========================================

# Prefix notation: (op arg1 arg2)
(assert-eq (+ 10 5) 15 "addition")          # (+ a b) not a + b
(assert-eq (- 10 3) 7 "subtraction")
(assert-eq (* 6 7) 42 "multiplication")
(assert-eq (/ 10 2) 5 "division")
(assert-eq (mod 10 3) 1 "modulo")           # remainder after division
(assert-eq (% 10 3) 1 "% is mod alias")     # % is shorthand for mod

# + and * accept any number of arguments
(display "  (+ 1 2 3 4) = ") (print (+ 1 2 3 4))   # 10
(display "  (* 1 2 3 4) = ") (print (* 1 2 3 4))    # 24
(assert-eq (+ 1 2 3 4) 10 "+ is variadic")
(assert-eq (* 1 2 3 4) 24 "* is variadic")

# Integer division truncates; float division doesn't
(display "  (/ 7 2)   = ") (print (/ 7 2))          # 3 (truncated)
(display "  (/ 7.0 2) = ") (print (/ 7.0 2))        # 3.5
(assert-eq (/ 7 2) 3 "int / int = int (truncates)")
(assert-eq (/ 7.0 2) 3.5 "float / int = float")

# Utility functions
(assert-eq (abs -5) 5 "abs")                # absolute value
(assert-eq (min 3 1 4 1 5) 1 "min is variadic")
(assert-eq (max 3 1 4 1 5) 5 "max is variadic")
(assert-true (even? 4) "even?")             # predicate: name ends in ?
(assert-true (odd? 3) "odd?")
(assert-true (even? 0) "0 is even")


# ========================================
# 4. Math
# ========================================

# math/ prefix for transcendental functions
(display "  (math/sqrt 16) = ") (print (math/sqrt 16))    # 4.0
(display "  (math/pow 2 10) = ") (print (math/pow 2 10))  # 1024
(display "  (math/pi) = ") (print (math/pi))               # 3.14159...
(assert-eq (math/sqrt 16) 4.0 "sqrt returns float")   # always returns float
(assert-eq (math/floor 3.7) 3 "floor returns integer") # round down → int
(assert-eq (math/ceil 3.2) 4 "ceil returns integer")   # round up → int
(assert-eq (math/round 3.5) 4 "round returns integer") # nearest → int
(assert-eq (math/pow 2 10) 1024 "pow")                 # 2^10
(assert-eq (math/sin 0) 0.0 "sin returns float")
(assert-eq (math/cos 0) 1.0 "cos returns float")
(assert-true (> (math/pi) 3.14) "pi > 3.14")
(assert-true (< (math/pi) 3.15) "pi < 3.15")


# ========================================
# 5. Comparison and logic
# ========================================

# = is structural equality (works on any type)
(assert-true (= 1 1) "= on equal ints")
(assert-false (= 1 2) "= on unequal ints")
(assert-true (< 1 2) "<")           # less than
(assert-true (> 2 1) ">")           # greater than
(assert-true (<= 1 1) "<= equal")   # less or equal
(assert-true (>= 2 2) ">= equal")   # greater or equal

(assert-true (not false) "not false")      # logical negation
(assert-false (not 0) "not 0 (0 is truthy)")  # 0 is truthy, so (not 0) = false

# and/or short-circuit and return the deciding value (not always a boolean)
(display "  (and 1 2 3)       = ") (print (and 1 2 3))       # 3
(display "  (and 1 false 3)   = ") (print (and 1 false 3))   # false
(display "  (or nil false 42) = ") (print (or nil false 42))  # 42
(assert-eq (and 1 2 3) 3 "and: returns last if all truthy")
(assert-eq (and 1 false 3) false "and: returns first falsy")  # stops at false
(assert-eq (or nil false 42) 42 "or: returns first truthy")   # skips nil, false
(assert-eq (or 0 1) 0 "or: 0 is truthy, returned first")     # 0 is truthy!


# ========================================
# 6. Bitwise
# ========================================

# bit/ prefix for bitwise operations on integers
(assert-eq (bit/and 12 10) 8 "bit/and")   # 1100 & 1010 = 1000
(assert-eq (bit/or 12 10) 14 "bit/or")    # 1100 | 1010 = 1110
(assert-eq (bit/xor 12 10) 6 "bit/xor")   # 1100 ^ 1010 = 0110
(assert-eq (bit/not 0) -1 "bit/not 0")    # ~0 = all ones = -1
(assert-eq (bit/shl 1 3) 8 "bit/shl")     # 1 << 3 = 8
(assert-eq (bit/shr 16 2) 4 "bit/shr")    # 16 >> 2 = 4

# Build a byte from nibbles: 0xA5 = (10 << 4) | 5 = 165
(display "  0xA5 = (10 << 4) | 5 = ") (print (bit/or (bit/shl 10 4) 5))
(assert-eq (bit/or (bit/shl 10 4) 5) 165 "nibble assembly")


# ========================================
# 7. Type conversions
# ========================================

# number->string and back
(display "  42 → \"") (display (number->string 42)) (print "\"")
(display "  \"42\" → ") (print (string->integer "42"))
(assert-eq (number->string 42) "42" "number->string int")
(assert-eq (string->integer "42") 42 "string->integer")     # parse string → int
(assert-eq (string->float "3.14") 3.14 "string->float")     # parse string → float

# Generic converters — named after the target type
(assert-eq (integer 3.7) 3 "integer truncates float")  # truncates, doesn't round
(assert-eq (float 42) 42.0 "float from int")           # widens to float

# Symbol/keyword → string
(assert-eq (symbol->string 'hello) "hello" "symbol->string")
(assert-eq (keyword->string :hello) "hello" "keyword->string (no colon)")

# Round-trip: int → string → int
(assert-eq (string->integer (number->string 99)) 99 "round-trip int")


# ========================================
# 8. The @ mutability split
# ========================================

# @ is the universal mutability prefix:
#   [...]  tuple  (immutable)    @[...]  array  (mutable)
#   {...}  struct (immutable)    @{...}  table  (mutable)
#   "..."  string (immutable)    @"..."  buffer (mutable)

(display "  [1 2 3]  → ") (print (type [1 2 3]))     # :tuple
(display "  @[1 2 3] → ") (print (type @[1 2 3]))    # :array
(display "  {:a 1}   → ") (print (type {:a 1}))      # :struct
(display "  @{:a 1}  → ") (print (type @{:a 1}))     # :table
(assert-eq (type [1 2 3]) :tuple "[] is tuple")       # immutable indexed
(assert-eq (type @[1 2 3]) :array "@[] is array")      # mutable indexed
(assert-eq (type {:a 1}) :struct "{} is struct")       # immutable keyed
(assert-eq (type @{:a 1}) :table "@{} is table")       # mutable keyed
(assert-eq (type "hello") :string "\"\" is string")    # immutable text
(assert-eq (type @"hello") :buffer "@\"\" is buffer")   # mutable text

# Mutable types are NOT their immutable counterparts
(assert-false (tuple? @[1 2]) "array is not tuple")
(assert-false (array? [1 2]) "tuple is not array")


# ========================================
# 9. Bytes and blobs
# ========================================

# bytes (immutable) and blob (mutable) — raw binary data
(def b (bytes 72 101 108 108 111))   # "Hello" in ASCII
(display "  (bytes 72 101 108 108 111) → \"") (display (bytes->string b)) (print "\"")
(display "  hex: ") (print (bytes->hex b))
(assert-eq (length b) 5 "bytes length")
(assert-eq (get b 0) 72 "get returns integer byte value")  # not a char
(assert-eq (bytes->string b) "Hello" "bytes->string (UTF-8)")

# each over bytes yields integers (byte values)
(var byte-sum 0)
(each v in (bytes 1 2 3)
  (set byte-sum (+ byte-sum v)))    # 1 + 2 + 3 = 6
(assert-eq byte-sum 6 "each over bytes sums integers")

# Conversions: string ↔ bytes ↔ blob ↔ buffer
(def b2 (string->bytes "hi"))        # string → bytes
(assert-eq (bytes->string b2) "hi" "round-trip string->bytes->string")
(def bl2 (bytes->blob b2))          # bytes → blob (mutable copy)
(assert-eq (type bl2) :blob "bytes->blob")
(def buf (bytes->buffer (string->bytes "test")))  # string → bytes → buffer
(assert-eq (type buf) :buffer "bytes->buffer")

# Slice — half-open range [start, end)
(def sliced (slice (bytes 10 20 30 40 50) 1 3))  # bytes at index 1,2
(assert-eq (length sliced) 2 "slice bytes length")
(assert-eq (get sliced 0) 20 "slice bytes content")


# ========================================
# 10. Boxes
# ========================================

# box/unbox/rebox — explicit first-class mutable cells
(def b (box 0))                      # create a box holding 0
(assert-true (box? b) "box? on box")
(assert-eq (unbox b) 0 "unbox initial value")   # read the box

(rebox b 42)                         # write a new value
(assert-eq (unbox b) 42 "rebox updates value")

# Boxes are different from var/set — they're first-class values.
# You can pass them around, store them in collections, share between closures.
(def counter (box 0))

(defn inc! []
  "Increment the shared counter."
  (rebox counter (+ (unbox counter) 1)))

(defn get-count []
  "Read the shared counter."
  (unbox counter))

(inc!)
(inc!)
(inc!)
(display "  counter after 3 increments: ") (print (get-count))
(assert-eq (get-count) 3 "shared counter via box")


# ========================================
# 11. Equality
# ========================================

# = does structural equality on data types
(assert-true (= [1 2 3] [1 2 3]) "= on equal tuples")          # same contents → equal
(assert-true (= @[1 2 3] @[1 2 3]) "= on equal arrays (structural)")
(assert-true (= {:a 1 :b 2} {:a 1 :b 2}) "= on equal structs")
(assert-true (= "hello" "hello") "= on equal strings")

# Closures compare by reference — two identical lambdas are NOT equal
(def f (fn [x] x))                  # a function
(def g (fn [x] x))                  # same code, different object
(assert-false (= f g) "identical lambdas are different objects")
(assert-true (= f f) "same closure is equal to itself")

# Destructuring works on any compound type
(def [a b c] [10 20 30])            # unpack a tuple into bindings
(display "  [10 20 30] → a=") (display a) (display " b=") (display b) (display " c=") (print c)
(assert-eq a 10 "tuple destructure: first")
(assert-eq c 30 "tuple destructure: third")

(def {:x px :y py} {:x 5 :y 10})    # unpack a struct by key
(display "  {:x 5 :y 10} → px=") (display px) (display " py=") (print py)
(assert-eq px 5 "struct destructure: x")
(assert-eq py 10 "struct destructure: y")

(print "")
(print "all basics passed.")
