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
#   Type conversions — number->string, integer, float, ...
#   Mutability split — [array] vs @[array], {struct} vs @{struct}, "str" vs @"str"
#   Bytes and @bytes  — immutable/mutable binary data
#   Boxes            — first-class mutable cells
#   Equality         — = does structural equality on data, reference on functions



# ========================================
# 1. Immediates
# ========================================

# nil — the absence of a value
(assert (nil? nil) "nil? on nil")         # nil is only equal to itself
(assert (not (nil? false)) "nil? on false")     # false is NOT nil
(assert (not (nil? 0)) "nil? on 0")            # 0 is NOT nil

# Booleans — true and false, that's it
(assert (boolean? true) "boolean? on true")
(assert (boolean? false) "boolean? on false")
(assert (not (boolean? 1)) "boolean? not on int")  # 1 is not a boolean

# Integers are 48-bit signed: -140,737,488,355,328 to 140,737,488,355,327.
# Floats are 64-bit IEEE 754 doubles.
# (Both are NaN-boxed into a single 8-byte Value representation.)
(assert (number? 42) "number? on int")     # integers are numbers
(assert (number? 3.14) "number? on float") # floats are numbers
(assert (= (type 42) :integer) "42 is integer")  # type returns a keyword
(assert (= (type 3.14) :float) "3.14 is float")
(assert (not (number? "42")) "number? not on string")  # "42" is a string

# Symbols and keywords — both are interned (fast equality)
(assert (symbol? 'hello) "symbol? on quoted symbol")  # 'x quotes a symbol
(assert (keyword? :hello) "keyword? on keyword")      # :x is a keyword
(assert (not (= 'name :name)) "symbol != keyword with same name")

# Empty list is its own thing — not nil
(assert (empty? (list)) "empty? on empty list")    # use empty? for lists
(assert (not (nil? (list))) "empty list is NOT nil")     # () and nil differ!

# (type x) returns a keyword describing x's type
(print "  (type 42)   = ") (println (type 42))       # :integer
(print "  (type 3.14) = ") (println (type 3.14))     # :float
(print "  (type \"hi\") = ") (println (type "hi"))    # :string
(print "  (type true) = ") (println (type true))     # :boolean
(print "  (type nil)  = ") (println (type nil))      # :nil
(print "  (type :foo) = ") (println (type :foo))     # :keyword
(print "  (type 'foo) = ") (println (type 'foo))     # :symbol
(assert (= (type 42) :integer) "type of int")
(assert (= (type "hi") :string) "type of string")
(assert (= (type nil) :nil) "type of nil")


# ========================================
# 2. Truthiness
# ========================================

# Only nil and false are falsy — everything else is truthy.
# This differs from C/Python/JS where 0, "", [] are falsy.
#
# (if test then else) evaluates test; if truthy, returns then, otherwise else.
# else is optional and defaults to nil. if is an expression — it returns a value.
(assert (not (if nil true false)) "nil is falsy")        # nil → else branch
(assert (not (if false true false)) "false is falsy")    # false → else branch

(assert (if 0 true false) "0 is truthy")           # unlike C/Python
(assert (if "" true false) "empty string is truthy")
(assert (if (list) true false) "empty list () is truthy")  # unlike nil!
(assert (if [] true false) "empty array is truthy")
(assert (if @[] true false) "empty array is truthy")
(assert (if :keyword true false) "keyword is truthy")
(assert (if 'symbol true false) "symbol is truthy")

(print "  falsy:  nil=") (print (if nil :T :F))
  (print " false=") (println (if false :T :F))
(print "  truthy: 0=") (print (if 0 :T :F))
  (print " \"\"=") (print (if "" :T :F))
  (print " ()=") (print (if (list) :T :F))
  (print " []=") (println (if [] :T :F))


# ========================================
# 3. Arithmetic
# ========================================

# Prefix notation: (op arg1 arg2)
(assert (= (+ 10 5) 15) "addition")          # (+ a b) not a + b
(assert (= (- 10 3) 7) "subtraction")
(assert (= (* 6 7) 42) "multiplication")
(assert (= (/ 10 2) 5) "division")
(assert (= (mod 10 3) 1) "modulo")           # remainder after division
(assert (= (% 10 3) 1) "% is mod alias")     # % is shorthand for mod

# + and * accept any number of arguments
(print "  (+ 1 2 3 4) = ") (println (+ 1 2 3 4))   # 10
(print "  (* 1 2 3 4) = ") (println (* 1 2 3 4))    # 24
(assert (= (+ 1 2 3 4) 10) "+ is variadic")
(assert (= (* 1 2 3 4) 24) "* is variadic")

# Integer division truncates; float division doesn't
(print "  (/ 7 2)   = ") (println (/ 7 2))          # 3 (truncated)
(print "  (/ 7.0 2) = ") (println (/ 7.0 2))        # 3.5
(assert (= (/ 7 2) 3) "int / int = int (truncates)")
(assert (= (/ 7.0 2) 3.5) "float / int = float")

# Utility functions
(assert (= (abs -5) 5) "abs")                # absolute value
(assert (= (min 3 1 4 1 5) 1) "min is variadic")
(assert (= (max 3 1 4 1 5) 5) "max is variadic")
(assert (even? 4) "even?")             # predicate: name ends in ?
(assert (odd? 3) "odd?")
(assert (even? 0) "0 is even")


# ========================================
# 4. Math
# ========================================

# math/ prefix for transcendental functions
(print "  (math/sqrt 16) = ") (println (math/sqrt 16))    # 4.0
(print "  (math/pow 2 10) = ") (println (math/pow 2 10))  # 1024
(print "  (math/pi) = ") (println (math/pi))               # 3.14159...
(assert (= (math/sqrt 16) 4.0) "sqrt returns float")   # always returns float
(assert (= (math/floor 3.7) 3) "floor returns integer") # round down → int
(assert (= (math/ceil 3.2) 4) "ceil returns integer")   # round up → int
(assert (= (math/round 3.5) 4) "round returns integer") # nearest → int
(assert (= (math/pow 2 10) 1024) "pow")                 # 2^10
(assert (= (math/sin 0) 0.0) "sin returns float")
(assert (= (math/cos 0) 1.0) "cos returns float")
(assert (> (math/pi) 3.14) "pi > 3.14")
(assert (< (math/pi) 3.15) "pi < 3.15")


# ========================================
# 5. Comparison and logic
# ========================================

# = is structural equality (works on any type)
(assert (= 1 1) "= on equal ints")
(assert (not (= 1 2)) "= on unequal ints")
(assert (< 1 2) "<")           # less than
(assert (> 2 1) ">")           # greater than
(assert (<= 1 1) "<= equal")   # less or equal
(assert (>= 2 2) ">= equal")   # greater or equal

(assert (not false) "not false")      # logical negation
(assert (not (not 0)) "not 0 (0 is truthy)")  # 0 is truthy, so (not 0) = false

# and/or short-circuit and return the deciding value (not always a boolean)
(print "  (and 1 2 3)       = ") (println (and 1 2 3))       # 3
(print "  (and 1 false 3)   = ") (println (and 1 false 3))   # false
(print "  (or nil false 42) = ") (println (or nil false 42))  # 42
(assert (= (and 1 2 3) 3) "and: returns last if all truthy")
(assert (= (and 1 false 3) false) "and: returns first falsy")  # stops at false
(assert (= (or nil false 42) 42) "or: returns first truthy")   # skips nil, false
(assert (= (or 0 1) 0) "or: 0 is truthy, returned first")     # 0 is truthy!


# ========================================
# 6. Bitwise
# ========================================

# bit/ prefix for bitwise operations on integers
(assert (= (bit/and 12 10) 8) "bit/and")   # 1100 & 1010 = 1000
(assert (= (bit/or 12 10) 14) "bit/or")    # 1100 | 1010 = 1110
(assert (= (bit/xor 12 10) 6) "bit/xor")   # 1100 ^ 1010 = 0110
(assert (= (bit/not 0) -1) "bit/not 0")    # ~0 = all ones = -1
(assert (= (bit/shl 1 3) 8) "bit/shl")     # 1 << 3 = 8
(assert (= (bit/shr 16 2) 4) "bit/shr")    # 16 >> 2 = 4

# Build a byte from nibbles: 0xA5 = (10 << 4) | 5 = 165
(print "  0xA5 = (10 << 4) | 5 = ") (println (bit/or (bit/shl 10 4) 5))
(assert (= (bit/or (bit/shl 10 4) 5) 165) "nibble assembly")


# ========================================
# 7. Type conversions
# ========================================

# number->string and back
(print "  42 → \"") (print (number->string 42)) (println "\"")
(print "  \"42\" → ") (println (integer "42"))
(assert (= (number->string 42) "42") "number->string int")
(assert (= (integer "42") 42) "integer from string")          # parse string → int
(assert (= (float "3.14") 3.14) "float from string")          # parse string → float

# Generic converters — named after the target type
(assert (= (integer 3.7) 3) "integer truncates float")  # truncates, doesn't round
(assert (= (float 42) 42.0) "float from int")           # widens to float

# Symbol/keyword → string
(assert (= (symbol->string 'hello) "hello") "symbol->string")
(assert (= (string :hello) "hello") "string keyword (no colon)")

# Round-trip: int → string → int
(assert (= (integer (number->string 99)) 99) "round-trip int")


# ========================================
# 8. The @ mutability split
# ========================================

# @ is the universal mutability prefix:
#   [...]  array  (immutable)    @[...]  @array  (mutable)
#   {...}  struct (immutable)    @{...}  @struct (mutable)
#   "..."  string (immutable)    @"..."  @string (mutable)

(print "  [1 2 3]  → ") (println (type [1 2 3]))     # :array
(print "  @[1 2 3] → ") (println (type @[1 2 3]))    # :@array
(print "  {:a 1}   → ") (println (type {:a 1}))      # :struct
(print "  @{:a 1}  → ") (println (type @{:a 1}))     # :@struct
(assert (= (type [1 2 3]) :array) "[] is array")       # immutable indexed
(assert (= (type @[1 2 3]) :@array) "@[] is @array")      # mutable indexed
(assert (= (type {:a 1}) :struct) "{} is struct")       # immutable keyed
(assert (= (type @{:a 1}) :@struct) "@{} is @struct")       # mutable keyed
(assert (= (type "hello") :string) "\"\" is string")    # immutable text
(assert (= (type @"hello") :@string) "@\"\" is @string")   # mutable text

# Both mutable and immutable arrays are arrays
(assert (array? @[1 2]) "mutable array is an array")
(assert (array? [1 2]) "immutable array is an array")


# ========================================
# 9. Bytes and blobs
# ========================================

# bytes (immutable) and @bytes (mutable) — raw binary data
(def b (bytes 72 101 108 108 111))   # "Hello" in ASCII
(print "  (bytes 72 101 108 108 111) → \"") (print (string b)) (println "\"")
(print "  hex: ") (println (bytes->hex b))
(assert (= (length b) 5) "bytes length")
(assert (= (get b 0) 72) "get returns integer byte value")  # not a char
(assert (= (string b) "Hello") "bytes->string (UTF-8)")

# each over bytes yields integers (byte values)
(var byte-sum 0)
(each v in (bytes 1 2 3)
  (assign byte-sum (+ byte-sum v)))    # 1 + 2 + 3 = 6
(assert (= byte-sum 6) "each over bytes sums integers")

# Conversions: string ↔ bytes ↔ @bytes ↔ @string
(def b2 (bytes "hi"))                # string → bytes
(assert (= (string b2) "hi") "round-trip string->bytes->string")
(def bl2 (thaw b2))                  # bytes → @bytes (mutable copy)
(assert (= (type bl2) :@bytes) "thaw bytes to @bytes")
(def buf (thaw "test"))               # string → @string (mutable copy)
(assert (= (type buf) :@string) "thaw string to @string")

# Slice — half-open range [start, end)
(def sliced (slice (bytes 10 20 30 40 50) 1 3))  # bytes at index 1,2
(assert (= (length sliced) 2) "slice bytes length")
(assert (= (get sliced 0) 20) "slice bytes content")


# ========================================
# 10. Boxes
# ========================================

# box/unbox/rebox — explicit first-class mutable cells
(def b (box 0))                      # create a box holding 0
(assert (box? b) "box? on box")
(assert (= (unbox b) 0) "unbox initial value")   # read the box

(rebox b 42)                         # write a new value
(assert (= (unbox b) 42) "rebox updates value")

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
(print "  counter after 3 increments: ") (println (get-count))
(assert (= (get-count) 3) "shared counter via box")


# ========================================
# 11. Equality
# ========================================

# = does structural equality on data types
(assert (= [1 2 3] [1 2 3]) "= on equal arrays")          # same contents → equal
(assert (= @[1 2 3] @[1 2 3]) "= on equal arrays (structural)")
(assert (= {:a 1 :b 2} {:a 1 :b 2}) "= on equal structs")
(assert (= "hello" "hello") "= on equal strings")

# Closures compare by reference — two identical lambdas are NOT equal
(def f (fn [x] x))                  # a function
(def g (fn [x] x))                  # same code, different object
(assert (not (= f g)) "identical lambdas are different objects")
(assert (= f f) "same closure is equal to itself")

# Destructuring works on any compound type
(def [a b c] [10 20 30])            # unpack an array into bindings
(print "  [10 20 30] → a=") (print a) (print " b=") (print b) (print " c=") (println c)
(assert (= a 10) "array destructure: first")
(assert (= c 30) "array destructure: third")

(def {:x px :y py} {:x 5 :y 10})    # unpack a struct by key
(print "  {:x 5 :y 10} → px=") (print px) (print " py=") (println py)
(assert (= px 5) "struct destructure: x")
(assert (= py 10) "struct destructure: y")

(println "")
(println "all basics passed.")
