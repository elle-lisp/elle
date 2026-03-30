# Elle Language Reference

Elle is a Lisp with lexical scope, closures, and a signal system. This document is what you need to write correct Elle code.

---

## Critical Gotchas

Read these first. They are the most common sources of bugs.

### `nil` ≠ `()` — causes infinite loops

`nil` is falsy. `()` (empty list) is **truthy**. Lists terminate with `()`, not `nil`.

```lisp
(nil? ())    # => false
(empty? ())  # => true
```

**Always use `empty?` to check end-of-list.** If you use `nil?` as your loop termination condition, it will never trigger — `()` is not `nil` — and your recursion will never bottom out.

These are separate values because they are genuinely distinguishable: `()` is what you get when you remove the last element from `(x)` — it's an empty container, not the absence of value. Any abstraction that conflates them is open to error.

### `#` is comment, `;` is splice

```lisp
# This is a comment
;[1 2 3]        # Splice: spreads array into surrounding form
(f 1 ;[2 3] 4)  # => (f 1 2 3 4)
[1 ;[2 3] 4]    # => [1 2 3 4]  — works in collection literals too
```

### `assign` mutates; `set` creates a set value

```lisp
(var x 10)
(assign x 20)   # Correct: mutates x
(set x 20)      # Wrong: creates a set collection
```

### Only `nil` and `false` are falsy

```lisp
(if 0   :t :f)  # => :t  (0 is truthy)
(if ""  :t :f)  # => :t  (empty string is truthy)
(if ()  :t :f)  # => :t  (empty list is truthy)
(if nil :t :f)  # => :f
```

### `silence` is a compile-time declaration; `squelch` is a runtime function

```lisp
# silence: preamble inside a lambda body — constrains a parameter's signal
(fn [f x]
  (silence f)   # f must be completely silent at call sites
  (f x))

# squelch: primitive call — returns a NEW closure that catches signals at runtime
(let ((safe-f (squelch f :yield)))
  (safe-f x))

# squelch accepts any signal spec: keyword, set, array, list, or integer
(squelch f |:yield :error|)   # set of signals
(squelch f [:yield :error])   # array also works

# Wrong: squelch requires exactly 2 args
(squelch f)     # arity error
```

### `let` is parallel; `let*` is sequential — causes "undefined variable"

`let` evaluates all right-hand sides in the **outer** scope. Later bindings
cannot see earlier ones. Use `let*` when a binding depends on a previous one.

```lisp
# WRONG: y cannot see x — "undefined variable: x"
(let ((x 5) (y (* x 2)))
  y)

# RIGHT: let* makes x visible to y
(let* ((x 5) (y (* x 2)))
  y)   # => 10
```

Also works with array-style syntax:

```lisp
(let* [[url (parse-url input)]
       [host url:host]           # host depends on url
       [conn (tcp/connect host 80)]]  # conn depends on host
  conn)
```

If you get "undefined variable" inside a `let` and the variable is defined
on an earlier line of the same `let`, change `let` to `let*`.

### Bare delimiters = immutable; `@`-prefix = mutable

```lisp
[1 2 3]    # array (immutable)
@[1 2 3]   # @array (mutable)
{:a 1}     # struct (immutable)
@{:a 1}    # @struct (mutable)
"hello"    # string (immutable)
@"hello"   # @string (mutable; get/put/length/push/pop are grapheme-indexed)
|1 2 3|    # set (immutable)
@|1 2 3|   # @set (mutable)
```

`put` on an immutable collection returns a new copy. `put` on a mutable collection mutates in place.

`freeze` and `thaw` convert between mutable and immutable forms for all collection types (strings, arrays, structs, sets). Both are shallow — nested collections keep their original mutability.

### Elle has no `-e` flag and no `-` flags at all

```bash
echo '(+ 1 2)' | elle        # run one-liner
elle script.lisp              # run file
elle                          # REPL
```

---

## Syntax

### Literals

```lisp
nil                  # absence of value (falsy)
true  false          # booleans (not #t/#f)
42                   # integer (64-bit signed)
3.14                 # float
0xFF  0o755  0b1010  # hex, octal, binary
1_000_000            # underscores ok
:keyword             # self-evaluating keyword
'symbol              # quoted symbol
()                   # empty list (truthy)
```

### Collections

```lisp
[1 2 3]              # array
@[1 2 3]             # @array (mutable)
{:a 1 :b 2}          # struct
@{:a 1 :b 2}         # @struct (mutable)
"hello"              # string
@"hello"             # @string (mutable; get/put/length/push/pop are grapheme-indexed)
|1 2 3|              # set
@|1 2 3|             # @set (mutable)
(bytes 1 2 3)        # immutable bytes
(@bytes 1 2 3)       # mutable bytes
```

### Quoting

```lisp
'(+ 1 2)             # quote — prevents evaluation
`(+ 1 ,x)            # quasiquote — unquote with ,
`(list ,;items)      # unquote-splice with ,;
```

---

## Binding and Scope

```lisp
# Top-level immutable (top-level is under implicit letrec,
# so defs may be mutually recursive without special declaration)
(def x 42)

# Top-level mutable
(var x 42)
(assign x 100)

# Local (parallel bindings — each RHS sees outer scope)
(let ((x 10) (y 20))
  (+ x y))

# Sequential (each binding sees previous)
(let* ((x 5) (y (* x 2)))
  (+ x y))   # => 15

# Recursive
(letrec ((f (fn [] (g)))
         (g (fn [] 42)))
  (f))
```

### Destructuring

Destructuring is strict — missing elements or keys signal an error (no silent nil). Extra elements are silently ignored. Works in all binding forms: `def`, `var`, `let`, `let*`, `fn`, `defn`, `match`.

```lisp
# List
(def (a b c) (list 1 2 3))
(def (head & tail) (list 1 2 3))   # head=1, tail=(2 3)

# Array (& rest collects into an array, not a list)
(def [x y] [10 20])
(def [first & rest] [1 2 3])       # rest = [2 3]

# Struct
(def {:x x :y y} {:x 5 :y 10})

# Struct remainder — & collects unmatched keys into a struct
(def {:a a & more} {:a 1 :b 2 :c 3})  # a=1, more={:b 2 :c 3}

# Wildcard — _ discards the matched value
(def (_ mid _) (list 10 20 30))    # mid=20

# Nested — any depth, mixed types
(def ((a b) c) (list (list 1 2) 3))
(def {:point [_ y]} {:point [:skip :target]})   # y=:target

# In function parameters
(defn magnitude [{:x x :y y}]
  (+ x y))

# Mutable destructuring
(var (a b) (list 1 2))
(assign a 100)
```

---

## Functions

```lisp
# Anonymous
(fn [x y] (+ x y))

# Named (defn is a macro)
(defn add [x y] (+ x y))

# With docstring
(defn add [x y]
  "Add two numbers."
  (+ x y))

# Variadic
(defn sum [& nums]
  (fold + 0 nums))

# Closures capture lexical environment
(defn make-adder [n]
  (fn [x] (+ x n)))

(def add5 (make-adder 5))
(add5 10)   # => 15

# Optional positional params (nil if omitted)
(defn greet [name &opt greeting]
  (println (or greeting "Hello") ", " name "!"))
(greet "Alice")         # Hello, Alice!
(greet "Bob" "Hey")     # Hey, Bob!

# Named keyword params (nil if omitted)
(defn connect [host port &named timeout]
  [host port timeout])
(connect "localhost" 8080 :timeout 30)  # => ["localhost" 8080 30]
(connect "localhost" 8080)              # => ["localhost" 8080 nil]

# Keyword args collected as a struct
(defn request [method path &keys opts]
  [method path opts])
(request "GET" "/" :timeout 30 :headers {:accept "text/html"})
# => ["GET" "/" {:timeout 30 :headers {:accept "text/html"}}]
```

---

## Control Flow

```lisp
# Conditional
(if test then else)

# Multi-branch
(cond
  ((> x 10) :large)
  ((> x 0)  :small)
  (true     :zero))

# One-armed
(when   test body...)
(unless test body...)

# Equality dispatch
(case x
  1 :one
  2 :two
  :other)

# Pattern matching (compiler errors on non-exhaustive match;
# any unbound symbol works as a wildcard, not just _)
(match value
  ([a b c] (+ a b c))
  ({:x x}  x)
  (_        :default))

# Sequencing (shares surrounding scope — useful in macros to collect
# expressions without introducing a new lexical scope and its attendant
# automatic memory management machinery)
(begin expr1 expr2 ...)

# Sequencing with a new scope
(block
  (var x 10)   # x is local to this block
  x)

# Named block with early exit
(block :label
  (each item in items
    (when (= item target)
      (break :label item)))
  nil)

# Loops — (break) exits while/forever directly; no label needed
(while (< i 10)
  (assign i (+ i 1)))

(forever
  (process)
  (when done (break)))

(repeat 5 (println "hi"))  # run body N times

# break does not cross function boundaries

# Iteration (prelude macro; `in` is optional sugar)
(each x in [1 2 3]
  (println x))
(each x [1 2 3]
  (println x))        # equivalent
```

---

## Error Handling

```lisp
# Raise
(error {:error :bad-input :message "expected a number"})

# Catch — e is a struct with :error (keyword) and :message (string)
(try
  (risky-op)
  (catch e
    (get e :error)    # => :division-by-zero etc.
    (get e :message)  # => "division by zero" etc.
  ))

# Capture as data — returns [ok? value]
# protect captures errors as data; try/catch affords a body for control flow
(def [ok? val] (protect (/ 10 0)))

# Protect + bind in one step — runs body only if expr succeeds
(when-ok [result (parse-json input)]
  (println "parsed:" result))
# Returns nil if expr errors (body is skipped)

# Guaranteed cleanup
(defer (close f)
  (use f))

# Resource management — with is defer with a constructor binding
(with f (open "data.txt") close
  (read-all f))
```

---

## Collections Reference

### Lists (linked)

```lisp
(list 1 2 3)
(cons 1 (list 2 3))
(first lst)   (rest lst)   (last lst)
(length lst)
(append lst1 lst2)
(reverse lst)
(take n lst)  (drop n lst)  (butlast lst)
(empty? lst)   # use this, not nil?
```

### Arrays (immutable, indexed)

```lisp
(length [1 2 3])       # => 3
(get [1 2 3] 1)        # => 2
(put [1 2 3] 0 99)     # => [99 2 3]  (new array)
```

### @Arrays (mutable, indexed)

```lisp
(length @[1 2 3])
(get @[1 2 3] 1)
(put @[1 2 3] 0 99)    # mutates in place
(push @[1 2 3] 4)      # appends, returns same object
(pop @[1 2 3])         # removes and returns last element
```

### Structs (immutable)

```lisp
(get {:a 1} :a)              # => 1
(get {:a 1} :b :default)     # => :default
(put {:a 1} :b 2)            # => {:a 1 :b 2}  (new struct)
(del {:a 1 :b 2} :a)         # => {:b 2}  (new struct)
(keys {:a 1 :b 2})           # => (:a :b)
(values {:a 1 :b 2})         # => (1 2)
(has? {:a 1} :a)             # => true
(length {:a 1 :b 2})         # => 2
(merge {:x 1 :y 2} {:y 3 :z 4})  # => {:x 1 :y 3 :z 4}
(update {:count 5} :count inc)   # => {:count 6}
(from-pairs [[:a 1] [:b 2]])     # => {:a 1 :b 2}

# Nested access and update (works on structs and arrays)
(get-in {:a {:b 1}} [:a :b])          # => 1
(put-in {:a {:b 1}} [:a :b] 2)        # => {:a {:b 2}}
(update-in {:a {:b 5}} [:a :b] inc)   # => {:a {:b 6}}
```

### @Structs (mutable)

```lisp
(put @{:a 1} :b 2)     # mutates in place
(del @{:a 1 :b 2} :a)  # mutates in place
```

### Strings (immutable, grapheme-indexed)

```lisp
(length "hello")              # => 5
(get "hello" 0)               # => "h"  (grapheme cluster)
(slice "hello" 1 4)           # => "ell"
(string/join ["a" "b"] "-")   # => "a-b"
(string/split "a,b" ",")      # => ["a" "b"]  (array)
(string/find "hello" "ll")    # => 2  (or nil)
(string/contains? "hello" "ll")    # => true
(string/starts-with? "hello" "he") # => true
(string/ends-with? "hello" "lo")   # => true
(string/upcase "hello")       # => "HELLO"
(string/downcase "HELLO")     # => "hello"
(string/trim "  hi  ")        # => "hi"
(string/replace "foo-bar" "-" "_") # => "foo_bar"  (replaces all)
(string/repeat "-" 40)            # => "----...----"
```

**Byte length vs grapheme length:** `length` returns the grapheme count, not
the byte count. For byte-level size (e.g. protocol framing, I/O offsets), use
`string/size-of`:

```lisp
(length "hello\r\n")        # => 6  (\r\n is one grapheme cluster)
(string/size-of "hello\r\n") # => 7  (byte count)
(length "👋🏽")               # => 1  (one grapheme cluster)
(string/size-of "👋🏽")       # => 8  (4+4 bytes UTF-8)
```

**String concatenation:** `(string ...)` converts all arguments to strings and concatenates them. This is the preferred way to build strings:

```lisp
(string "hello " "world")     # => "hello world"
(string "count: " 42)         # => "count: 42"
(string "key:" :foo "=" val)  # => "key:foo=123"
```

`string/join` is for joining a collection with a separator:

```lisp
(string/join ["a" "b" "c"] ",")  # => "a,b,c"
```

`string` with multiple args concatenates, coercing non-strings:

```lisp
(string "a" 3 "b")                     # => "a3b"
(string/format "{} + {} = {}" 1 2 3)   # => "1 + 2 = 3"
```

### @Strings (mutable)

`get`, `put`, `length`, `push`, and `pop` are all grapheme-indexed, consistent with immutable strings.

```lisp
(thaw "hello")         # string → @string
(freeze @"hello")      # @string → string
(get @"hello" 0)       # => "h"  (grapheme cluster as string)
(put @"hello" 0 "H")   # replaces grapheme at index, returns @string
(length @"hello")      # => 5  (grapheme count)
(push @"hello" "!")    # appends string, returns @string
(pop @"hello")         # removes and returns last grapheme cluster as string
```

### Sets

```lisp
|1 2 3|                      # immutable set
@|1 2 3|                     # mutable set
(contains? |1 2 3| 2)        # => true
(add @|1 2 3| 4)             # mutates in place
(del @|1 2 3| 1)             # mutates in place
(union s1 s2)
(intersection s1 s2)
(difference s1 s2)
```

### Bytes

```lisp
(bytes 1 2 3)
(bytes "hello")              # string → bytes (UTF-8)
(get (bytes 1 2 3) 0)        # => 1
(length (bytes 1 2 3))       # => 3
(seq->hex (bytes 1 2 3))     # => "010203"  (canonical name)
(bytes->hex (bytes 1 2 3))   # => "010203"  (alias for seq->hex)
(seq->hex [1 2 3])           # => "010203"  (also works with arrays)
(seq->hex 255)               # => "ff"      (and non-negative integers)
(seq->hex 0)                 # => "00"
(string (bytes 97 98 99))    # => "abc"
```

### Boxes (mutable cells)

```lisp
(box 42)
(unbox b)
(rebox b 99)
```

---

## Higher-Order Functions

```lisp
(map    f [1 2 3])           # => (2 4 6)  — returns list
(filter f [1 2 3 4])         # => (3 4)    — returns list
(fold   f init [1 2 3])      # => 10
(apply  f [1 2 3])           # => (f 1 2 3)
(sum [1 2 3 4])              # => 10
(product [1 2 3 4])          # => 24

# Type conversion
(->array (list 1 2 3))      # => [1 2 3]
(->list [1 2 3])             # => (1 2 3)
(->array "abc")              # => ["a" "b" "c"]

# Mutability conversion
(freeze @[1 2 3])            # => [1 2 3]   (shallow)
(thaw [1 2 3])               # => @[1 2 3]  (shallow)
(deep-freeze @[@[1] @{:a 2}])  # => [[1] {:a 2}]  (recursive)

# Threading macros
(-> 5 (+ 10) (* 2))          # => 30  (insert as first arg)
(->> [1 2 3] (map double))   # (insert as last arg)
```

Note: `map` and `filter` always return lists, even when given arrays.

### Sorting

```lisp
(sort [3 1 2])                    # => (1 2 3)  — natural order
(sort-by length ["bb" "a" "ccc"]) # => ("a" "bb" "ccc")  — by key function
(sort-with (fn [a b] (> a b)) [3 1 2])  # => (3 2 1)  — custom comparator
```

---

## Arithmetic and Math

```lisp
(+ 1 2 3)   (- 10 3)   (* 2 3 4)
(/ 10 2)               # integer division truncates
(/ 7.0 2)              # => 3.5  (mixed: promotes to float)
(/ 1.0 0.0)            # => inf  (IEEE 754; integer /0 errors)
(mod 10 3)             # => 1   (flooring: sign follows divisor)
(rem 10 3)             # => 1   (truncating: sign follows dividend)
(mod -10 3)            # => 2
(rem -10 3)            # => -1
(abs -5)
(min 3 1 4)  (max 3 1 4)
(even? 4)    (odd? 3)

(math/sqrt 16)   (math/cbrt 27)   (math/pow 2 10)
(math/floor 3.7) (math/ceil 3.2)  (math/round 3.5)  (math/trunc 3.7)
(math/sin x)     (math/cos x)     (math/tan x)
(math/asin x)    (math/acos x)    (math/atan x)     (math/atan2 y x)
(math/sinh x)    (math/cosh x)    (math/tanh x)
(math/exp x)     (math/exp2 x)
(math/log x)     (math/log2 x)    (math/log10 x)
(math/pi)        (math/e)
(math/inf)       (math/-inf)      (math/nan)
# All math/ functions have bare aliases: sqrt, sin, log2, +inf, -inf, nan, etc.

(bit/and 12 10)  (bit/or 12 10)  (bit/xor 12 10)
(bit/not 0)      (bit/shl 1 3)   (bit/shr 16 2)
```

### Comparison and Equality

```lisp
(= 1 1)         # => true  (structural equality)
(= 1 1.0)       # => true  (numeric coercion: int and float compare naturally)
(= [1 2] @[1 2]) # => true  (cross-mutability: same contents = equal)
(not= 1 2)       # => true  (inequality)
(< 1 2)         (> 2 1)
(< 3 3.5)       # => true  (mixed int/float)
(<= 1 1)        (>= 2 1)
(hash :foo)      # => integer  (deterministic hash of any value)
(= (hash 1) (hash 1.0))  # => true  (equal values hash identically)
```

### Logical Operators

```lisp
(and x y z)     # short-circuiting; returns last truthy or first falsy
(or x y z)      # short-circuiting; returns first truthy or last falsy
(not false)     # => true  (not is a function, not a macro)
```

---

## Type System

```lisp
(type-of 42)        # => :integer
(type-of 3.14)      # => :float
(type-of "hi")      # => :string
(type-of :kw)       # => :keyword
(type-of nil)       # => :nil
(type-of ())        # => :list
(type-of [1 2])     # => :array
(type-of {:a 1})    # => :struct
(type-of true)      # => :boolean
(type-of (fn [] 1)) # => :closure  (use fn? to test)
(type-of +)         # => :native-fn
(type-of ptr)       # => :ptr

# Predicates
(nil? x)      (boolean? x)   (number? x)
(integer? x)  (float? x)     (symbol? x)
(keyword? x)  (string? x)    (pair? x)
(list? x)     (empty? x)     (array? x)
(struct? x)   (bytes? x)     (set? x)
(box? x)      (fiber? x)
(fn? x)          # closure or native-fn (any callable)
(closure? x)     # user-defined closures only
(native-fn? x)   # native functions only; aliases: native?, primitive?
(mutable? x)     # true for @array, @struct, @string, @bytes, @set, box
(immutable? x)   # complement of mutable?
(zero? x)        # 0 or 0.0
(nonzero? x)     # complement of zero?
(nonempty? x)    # complement of empty?
(pos? x)         # number > 0
(neg? x)         # number < 0
(nan? x)         # float NaN
(inf? x)         # float infinity

# Conversions
(integer "42")             (float "3.14")
(integer "ff" 16)          # => 255  (hex, radix 2–36)
(integer "1010" 2)         # => 10   (binary)
(string 42)                (string 'foo)   # => "foo"
(string :foo)              # => "foo"  (no colon)
(string @"hello")          # => "hello"  (@string → string)
(number->string 42)        # => "42"  (decimal)
(number->string 255 16)    # => "ff"   (hex, radix 2–36)
(number->string 255 2)     # => "11111111"  (binary)
```

---

## Macros

Macros receive unevaluated syntax and return syntax to be evaluated.

### Quasiquote-based macros

Use quasiquote (`` ` ``), unquote (`,`), and unquote-splice (`,;`) to construct output:

```lisp
(defmacro when [test & body]
  `(if ,test (begin ,;body) nil))
```

### `syntax-case` — pattern-matching macros

For more precise structural matching, use `syntax-case`. It matches against the macro's input syntax and binds pattern variables:

```lisp
(defmacro swap! [a b]
  (syntax-case (list a b)
    ([x y]
     `(let ((tmp ,x))
        (assign ,x ,y)
        (assign ,y tmp)))))
```

Patterns:
- `_` — wildcard, matches anything
- `x` — pattern variable, binds the matched syntax
- `(a b)` — matches a 2-element list, binds `a` and `b`
- `(literal sym)` — matches the symbol `sym` literally
- Guards: `(pat when guard-expr body)`
- Multiple clauses: first match wins

---

## Signals and Fibers

Signals are the unified mechanism for all non-local control flow. Every signal is a bit in a mask.

**Built-in signals:** `:error` (bit 0), `:yield` (bit 1), `:debug` (bit 2), `:ffi` (bit 4), `:halt` (bit 8), `:io` (bit 9), `:exec` (bit 11), `:fuel` (bit 12).

```lisp
# Create a fiber (mask = SIG_YIELD = 2)
(def f (fiber/new (fn [] (yield 42)) 2))

# Resume, delivering a value; returns the next yielded value
(fiber/resume f nil)

# Inspect
(fiber/status f)   # :new :alive :suspended :dead :error
(fiber/value f)    # signal value
(fiber/bits f)     # signal bits
(fiber/mask f)     # signal mask

# Terminate
(fiber/cancel f)   # hard kill (no unwinding)
(fiber/abort f)    # graceful (with unwinding)

# The signal mask is fixed at fiber creation time.
# Uncaught signals propagate to the parent fiber.
# Uncaught errors crash the runtime — you don't need ev/join for propagation.

# Errors are NOT implicitly unwinding. A parent that catches an error
# can resume the errored fiber to offer restart functionality.

# Signal inference: the compiler infers signal types from the body.
# silence constrains a parameter to be silent at compile time.
# squelch wraps a closure at runtime to catch signals.
```

---

## I/O and Subprocesses

### Ports (not-thread-safe)

```lisp
(port/open "file.txt" :read)
(port/open "file.txt" :write)
(port/read p n)          # read n bytes
(port/write p bytes)
(port/close p)
(port/seek p offset)     # seek to byte offset (default: from start)
(port/seek p off :from :end)  # seek from end
(port/tell p)            # current byte position
(port/lines p)           # stream of lines
(port/chunks p n)        # stream of byte chunks
(port/writer p)          # writable stream
```

### Subprocess

```lisp
# Run to completion — returns {:exit int :stdout string :stderr string}
(subprocess/system "echo" ["hello"])
# => {:exit 0 :stdout "hello\n" :stderr ""}

# With options
(subprocess/system "ls" ["-la"] {:cwd "/tmp"})
(subprocess/system "env" [] {:env {:FOO "bar"}})

# Low-level
(def proc (subprocess/exec "cat" []))
(subprocess/pid proc)
(subprocess/wait proc)
(subprocess/kill proc)
```

### Streams

Streams are lazy and pull-based. Use `stream/map`, `stream/filter`, etc. to transform streams — the eager `map`/`filter` functions operate on lists, not streams.

```lisp
(stream/map    f stream)
(stream/filter f stream)
(stream/take   n stream)
(stream/drop   n stream)
(stream/concat s1 s2)
(stream/zip    s1 s2)
(stream/for-each f stream)
(stream/fold   f init stream)
(stream/collect stream)       # => list
(stream/into-array stream)    # => array
(stream/pipe src dst)
```

### Async I/O and Structured Concurrency

User code runs inside the async scheduler automatically. Port I/O
(`port/write`, `port/read`, etc.) yields to the scheduler — no setup needed.

**Spawn and join** — the fundamental building blocks:

```lisp
# Spawn a fiber, wait for its result
(def f (ev/spawn (fn [] (port/read-all (port/open "data.txt" :read)))))
(def content (ev/join f))

# Join a sequence — results in input order
(def [a b] (ev/join [(ev/spawn (fn [] (fetch "/users")))
                      (ev/spawn (fn [] (fetch "/posts")))]))
```

**Parallel map** — the most common pattern:

```lisp
(ev/map (fn [url] (http/get url)) urls)   # → [response1 response2 ...]
```

**Error handling** — `ev/join-protected` returns `[ok? value]` instead of
raising errors:

```lisp
(let (([ok? val] (ev/join-protected (ev/spawn (fn [] (flaky-api-call))))))
  (if ok? val (cached-fallback)))
```

**Select, race, timeout** — waiting for the first of N:

```lisp
# First to complete wins; abort the rest
(ev/race [(ev/spawn (fn [] (query-replica-1)))
          (ev/spawn (fn [] (query-replica-2)))])

# Deadline on a computation
(ev/timeout 5 (fn [] (http/get "https://slow-api.example.com")))
```

**Scoped concurrency** — all children must finish before scope exits:

```lisp
(ev/scope (fn [spawn]
  (let ([users    (spawn (fn [] (fetch "/users")))]
        [settings (spawn (fn [] (fetch "/settings")))])
    # If /settings fails, /users is aborted automatically
    {:users (ev/join users) :settings (ev/join settings)})))
```

Key primitives:

```lisp
(ev/spawn thunk)            # create a fiber, returns fiber handle
(ev/join fiber-or-seq)      # wait for result(s), propagate errors
(ev/join-protected target)  # wait without raising — returns [ok? value]
(ev/abort fiber)            # graceful cancel (defer blocks run)
(ev/select fibers)          # wait for first → [done remaining]
(ev/race fibers)            # first wins, abort rest, return value
(ev/timeout secs thunk)     # deadline — returns value or {:error :timeout}
(ev/scope (fn [spawn] ...)) # nursery — children can't outlive scope
(ev/map f items)            # parallel map, results in order
(ev/map-limited f items n)  # bounded parallel map (at most n in flight)
(ev/as-completed fibers)    # lazy iterator → [next-fn pool]
(ev/sleep seconds)          # yield for N seconds

(tcp/listen addr port)      # bind and listen, returns listener
(tcp/accept listener)       # yields until connection, returns port
(tcp/connect host port)     # yields until connected, returns port

(port/write port data)      # async write (yields)
(port/flush port)           # async flush (yields)
(port/read port n)          # async read N bytes (yields)
(port/read-line port)       # async read until \n (yields), nil on EOF
```

**Important:** `port/write` and `port/flush` signal `:yield`. `print`/`println`
and `eprint`/`eprintln` are also async — they write to `*stdout*`/`*stderr*`
via the same async I/O path.

### Channels

Crossbeam-based channels for inter-fiber (and inter-thread) messaging. Often unnecessary in single-threaded designs where a list or array suffices.

```lisp
(def [tx rx] (chan))            # unbounded channel; returns [sender receiver]
(def [tx rx] (chan 10))         # bounded (capacity 10)

(chan/send tx 42)               # non-blocking; returns [:ok], [:full], or [:disconnected]
(chan/recv rx)                  # non-blocking; returns [:ok msg], [:empty], or [:disconnected]

(chan/clone tx)                 # clone sender (multiple producers)
(chan/close tx)                 # close sender half
(chan/close-recv rx)            # close receiver half

# Multiplex: block until one receiver has data
(chan/select @[r1 r2])          # => [index msg] or [:disconnected]
(chan/select @[r1 r2] 1000)    # with timeout (ms); may return [:timeout]
```

### Threads

```lisp
(def handle (sys/spawn (fn [] (+ 1 2))))  # spawn OS thread
(sys/join handle)                          # wait for result; => 3
(sys/thread-id)                            # current thread ID
```

### Output

```lisp
(print "no newline")           # write to *stdout*, no newline
(println "with newline")       # write to *stdout* + newline
(println "count: " 42)         # multiple args concatenated
(println)                      # just a newline

(eprint "no newline")          # write to *stderr*, no newline
(eprintln "error: bad input")  # write to *stderr* + newline
```

All four respect `*stdout*`/`*stderr*` parameter rebinding.

### Dynamic Parameters

You can define your own dynamic parameters with `make-parameter`:

```lisp
(def *my-param* (make-parameter :default-value))
(*my-param*)                  # => :default-value
(parameter? *my-param*)       # => true

(parameterize ((*my-param* :overridden))
  (*my-param*))               # => :overridden

# Built-in parameters include *stdout* and *stderr*
(parameterize ((*stdout* my-port))
  (println "goes to my-port, not terminal"))
```

`pp` pretty-prints data structures in literal form (useful for debugging):

---

## Modules and Imports

```lisp
# Import by short name — searches ELLE_PATH, ELLE_HOME, and CWD
(def http ((import "lib/http")))       # finds lib/http.lisp
(def crypto (import "crypto"))         # finds libelle_crypto.so via ELLE_PATH

# Full path still works (backward compat)
(import "lib/http.lisp")
(import-file "lib/http.lisp")         # alias

# Plugin by full path
(def crypto (import "target/release/libelle_crypto.so"))
```

`import` resolves module specifiers by searching directories in order: CWD, `ELLE_PATH` (colon-separated), `ELLE_HOME` (defaults to the elle binary's directory). For each directory it probes: `<spec>` as-is, `<spec>.lisp`, `libelle_<leaf>.so`. Plugins are cached — re-importing the same `.so` returns the cached struct without re-loading.

Importing a plugin returns a struct of its functions. Bind it to use them — plugin functions are **not** injected into the global scope.

---

## Plugins

Plugins are Rust cdylib crates that extend Elle with new primitives. `import` loads the `.so` and returns a struct of its functions. Bind the result — functions are not injected into scope.

**Build plugins before use:**
```bash
cargo build --release -p elle-crypto
# produces target/release/libelle_crypto.so
```

**Pattern:**
```lisp
# With ELLE_PATH=target/release (or ELLE_HOME pointing to install dir):
(def crypto (import "crypto"))
(seq->hex (crypto:sha256 "hello"))

# Or destructure:
(def {:sha256 sha256 :hmac-sha256 hmac} (import "crypto"))
(seq->hex (sha256 "hello"))
# (bytes->hex is an alias for seq->hex and still works)
```

### Selected Plugins

Elle ships with 23+ plugins. Here are a few commonly used ones:

#### `elle-crypto` — SHA-2 hashing and HMAC

```lisp
(def crypto (import "target/release/libelle_crypto.so"))
# Keys: :sha224 :sha256 :sha384 :sha512
#       :hmac-sha224 :hmac-sha256 :hmac-sha384 :hmac-sha512
(seq->hex ((get crypto :sha256) "hello"))  # seq->hex is canonical
(seq->hex ((get crypto :hmac-sha256) "key" "message"))
```

#### `elle-glob` — Filesystem glob patterns

```lisp
(def glob (import "target/release/libelle_glob.so"))
# Keys: :glob :match? :match-path?
((get glob :glob) "src/**/*.rs")           # => list of matching paths
((get glob :match?) "*.lisp" "foo.lisp")   # => true
```

#### `elle-random` — Pseudo-random numbers

```lisp
(def random (import "target/release/libelle_random.so"))
# Keys: :int :float :bool :shuffle
((get random :int) 1 100)      # random integer in [1, 100]
((get random :float))          # random float in [0.0, 1.0)
((get random :shuffle) [1 2 3])
```

#### `elle-regex` — Regular expressions

```lisp
(def re (import "target/release/libelle_regex.so"))
# Keys: :compile :match? :find :find-all :split :replace
(def pat ((get re :compile) "\\d+"))
((get re :match?) pat "abc123")           # => true
((get re :find) pat "abc123def")          # => "123"
((get re :replace) pat "a1b2" "N")        # => "aNbN"
```

#### `elle-sqlite` — SQLite database

```lisp
(def sqlite (import "target/release/libelle_sqlite.so"))
# Keys: :open :query :exec :close
(def db ((get sqlite :open) "data.db"))
((get sqlite :exec) db "CREATE TABLE t (id INTEGER, name TEXT)")
((get sqlite :query) db "SELECT * FROM t")
# => list of structs: ({:id 1 :name "Alice"} ...)
((get sqlite :close) db)
```

#### `elle-hash` — Universal hashing

```lisp
(def hash (import "target/release/libelle_hash.so"))
# Keys: :md5 :sha1 :sha224 :sha256 :sha384 :sha512 :sha512-224 :sha512-256
#       :sha3-224 :sha3-256 :sha3-384 :sha3-512
#       :blake2b-512 :blake2s-256 :blake3 :blake3-keyed :blake3-derive
#       :crc32 :xxh32 :xxh64 :xxh128

(bytes->hex (hash:sha256 "hello"))
# => "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"

(bytes->hex (hash:blake3 "hello"))
# => "ea8f163db38682925e4491c5e58d4bb3506ef8c14eb78a86e908c5624a67200f"

(hash:crc32 "hello")
# => 907060870

(hash:xxh64 "hello")
# => integer

# Streaming (incremental) hashing
(let* [[ctx (hash:new :sha256)]]
  (hash:update ctx "hel")
  (hash:update ctx "lo")
  (bytes->hex (hash:finalize ctx)))
# => same as (bytes->hex (hash:sha256 "hello"))

# Compose with stream/fold and port/chunks
(let* [[ctx (stream/fold hash:update (hash:new :sha256) (port/chunks port 8192))]]
  (hash:finalize ctx))

# Or use the convenience wrapper (lib/hash.lisp)
(def h ((import-file "lib/hash.lisp") hash))
(bytes->hex (h:file :sha256 "bigfile.bin"))
```

#### `elle-selkie` — Mermaid diagram rendering

```lisp
(def selkie (import "target/release/libelle_selkie.so"))
# Keys: :render :render-to-file
((get selkie :render) "graph TD\n  A --> B")
((get selkie :render-to-file) "graph TD\n  A --> B" "out.svg")
```

#### `elle-tls` — TLS client and server via rustls

```lisp
(import "target/release/libelle_tls.so")
(def tls ((import-file "lib/tls.lisp")))
# Client: tls:connect, tls:read, tls:write, tls:read-all, tls:close
# Server: tls:server-config, tls:accept
(let ((conn (tls:connect "example.com" 443)))
  (defer (tls:close conn)
    (tls:write conn "GET / HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n")
    (println (string (tls:read-all conn)))))
```

#### `elle-tree-sitter` — Multi-language parsing and structural queries

```lisp
(def ts (import "target/release/libelle_tree_sitter.so"))
# Keys: :parse :query :root :children :text :kind :node-at :walk
(def tree ((get ts :parse) :rust "fn main() { 42 }"))
(def matches ((get ts :query) tree "(integer_literal) @num"))
```

### Plugin Gotchas

- `import` returns a **struct** — functions are accessed via `get`, not called by name.
- Plugins are **never unloaded** — the library handle is intentionally leaked.
- Plugins have **no stable ABI** — recompile when upgrading Elle.
- The analyzer has **no static knowledge** of plugin functions — no compile-time checking.
- Every `import` call **reloads** the plugin — plugins follow the same strict re-interpretation code-path as modules.

---

## Subprocess and System Args

```lisp
# Args after the source file on the command line (includes the literal "--")
# elle script.lisp -- foo bar baz  =>  ("--" "foo" "bar" "baz")
# elle script.lisp                 =>  ()
(def args (sys/args))
(def real-args (drop 1 args))   # skip the "--" separator

# Environment
(sys/env)              # => struct of all env vars as strings
(sys/env "HOME")       # => single var lookup, or nil if unset
```

**Example script using args:**
```lisp
# greet.lisp
(def args (drop 1 (sys/args)))   # skip "--"
(if (empty? args)
  (println "Usage: elle greet.lisp -- <name>")
  (println (string "Hello, " (first args) "!")))
```
```bash
elle greet.lisp -- Alice   # => Hello, Alice!
```

---

## Introspection and Help

Elle's API has three layers:

1. **VM primitives** — native functions implemented in Rust (`+`, `get`, `port/write`, etc.)
2. **stdlib.lisp** — standard library closures and macros (`map`, `filter`, `fold`, etc.)
3. **prelude.lisp** — higher-level abstractions (`ev/spawn`, `ev/join`, `ev/map`, `each`, `match`, etc.)

`vm/list-primitives` and `vm/primitive-meta` only cover layer 1 — they do NOT
list stdlib or prelude functions. If you don't find something in the primitive
list, **check stdlib.lisp and prelude.lisp in the project root** — they are
the source of truth for the full API.

`doc` works across all three layers:

```lisp
# doc works on primitives
(doc +)
# (+ xs)
#   Sum all arguments. Returns 0 for no arguments.
#   arity: 0+
#   example: (+ 1 2 3) #=> 6

# doc also works on prelude functions
(doc ev/spawn)  # => "Spawn a closure in a new fiber ..."
(doc ev/join)   # => "Wait for a fiber or sequence of fibers ..."
(doc each)      # => "(each (name list) body...) ..."

# vm/list-primitives lists ONLY native VM primitives, not stdlib/prelude
(vm/list-primitives)

# vm/primitive-meta returns metadata for a primitive
(vm/primitive-meta "+")
# => {:name "+" :doc "..." :arity "0+" :params ("xs") :signal "silent+errors"
#     :aliases () :category "arithmetic" :example "..."}

# To discover stdlib/prelude functions, read the source files:
#   stdlib.lisp   — map, filter, fold, append, reverse, etc.
#   prelude.lisp  — ev/spawn, ev/join, ev/map, each, match, cond, etc.
```

---

## Common Patterns

### Tail-recursive list processing

Tail call optimization is guaranteed — tail calls in Elle never grow the stack.

```lisp
(defn sum [lst acc]
  (if (empty? lst)
    acc
    (sum (rest lst) (+ acc (first lst)))))

(sum [1 2 3 4 5] 0)   # => 15
```

### Mutable accumulator

```lisp
# var captures in closures are semantically box-like
(defn make-counter []
  (var n 0)
  (fn []
    (assign n (+ n 1))
    n))
```

### Safe error capture

```lisp
(def [ok? val] (protect (risky-op)))
(if ok?
  (use val)
  (handle-error val))
```

### Struct update

```lisp
(update state :count inc)                    # apply function to one key
(merge state {:count (+ (get state :count) 1)})  # merge for multiple keys
(update-in config [:db :pool-size] inc)      # nested update
```

### Iterate with index

```lisp
(var i 0)
(each x in items
  (println i x)
  (assign i (+ i 1)))
```

---

## What Doesn't Exist

These are things agents commonly reach for that Elle does not have:

| Missing | Use instead |
|---------|-------------|
| `string-append` | `(string "a" x "b")` for concatenation; `string/join` for joining with separator |
| `-e` flag | `echo '...' \| elle` |
| `set!` / `set` for mutation | `assign` |
| `#t` / `#f` | `true` / `false` |
| `define` | `def` / `defn` / `var` |
| `lambda` | `fn` |
| `begin` as scoped sequencing | `block` (`begin` shares surrounding scope) |
| `display` | `print` (epoch 3 renamed `display` to `print`) |
| `write` (literal form) | `pp` for pretty-print, or `port/write` for port I/O |
| Mutable struct field update | `put` on `@struct` |
| `null` | `nil` |
| `char` type | `string` and `@string` are grapheme-indexed; use `get` to extract a grapheme cluster as a string |
| `function?` | `fn?` (callable), `closure?` (closure only), `native-fn?` (native only) |

---

## Deeper Understanding

### `silence` vs `squelch`

They solve different problems at different times:

- **`silence`** is a compile-time declaration. It appears in a function preamble and means "totally silent" — no signal keywords accepted. Two forms: `(silence)` constrains the whole function body; `(silence param)` constrains a parameter. If a `(silence)` function's body may emit signals, the **compiler rejects it** at compile time. If a `(silence param)` parameter emits a signal at runtime, it becomes a `:signal-violation` error.

- **`squelch`** is a runtime primitive that returns a **new closure** with a selective signal blacklist. It takes exactly two arguments: a closure and a signal spec (keyword, set, array, list, or integer). You can compose them: `(squelch (squelch f :yield) :error)`.

```lisp
# silence — total compile-time contract
(defn safe-map [f xs]
  (silence f)              # f must be completely silent
  (map f xs))

# squelch — selective runtime wrapper
(defn safe-iterate [f xs]
  (let ((safe-f (squelch f |:yield|)))  # forbid :yield only
    (map safe-f xs)))
```

| Aspect | `silence` | `squelch` |
|--------|-----------|-----------|
| Type | Special form (preamble) | Runtime primitive |
| Granularity | All-or-nothing | Per-signal keyword |
| Checking | Compile-time (body) / runtime (params) | Runtime |
| Returns | `nil` | New closure |

### `begin` vs `block`

`begin` is the default sequential form. `block` is for early exit.

- **`begin`** shares the surrounding scope. Inside a function body, it performs two-pass analysis (pre-binds all `def`/`var` forms, then analyzes), enabling forward references and mutual recursion. All multi-body macros (`when`, `unless`, `let*`, etc.) desugar to `begin`.

- **`block`** creates a new isolated scope and supports `break` for early exit with a value. It has more overhead (scope infrastructure, exit labels, region tracking).

```lisp
# begin — idiomatic for sequencing (used in macro expansion)
(when ready
  (begin
    (process)
    (cleanup)))

# block — needed for early exit
(block :search
  (each item in items
    (when (= item target)
      (break :search item)))
  nil)
```

Use `begin` unless you need `break`.

### Fiber signal masks

Signal masks accept multiple formats, but **set literals are preferred**:

```lisp
# Preferred — symbolic set literal
(fiber/new (fn [] (yield 42)) |:yield|)
(fiber/new thunk |:yield :io|)

# Also accepted: keyword, array, integer
(fiber/new thunk :yield)
(fiber/new thunk [:yield :io])
(fiber/new thunk 2)              # raw bit value (fragile)
```

Built-in signal bits: `:error` (0), `:yield` (1), `:debug` (2), `:ffi` (4), `:halt` (8), `:io` (9), `:exec` (11), `:fuel` (12), `:wait` (14). User-defined signals (via `(signal :keyword)`) get bits 16–31.

The runtime resolves set literals by looking up each keyword in the signal registry and OR-ing the bits together. Use `|:yield :io|` rather than `(bit/or 2 512)`.

### `import` reloads every time

There is no module cache. Every `import` call reads the file, compiles it, and executes it fresh. This is intentional — it gives each import independent state:

```lisp
# Two imports of the same counter module get independent state
(let ([c1 ((import-file "lib/counter.lisp"))]
      [c2 ((import-file "lib/counter.lisp"))])
  (c1:inc) (c1:inc)
  (c1:count)   # => 2
  (c2:count))  # => 0
```

**To avoid redundant loads**, bind the result at the top level and reuse it:

```lisp
(def utils (import-file "lib/utils.lisp"))
(utils:helper1 x)
(utils:helper2 y)
```

For plugins (`.so` files), the library handle is intentionally leaked (never unloaded). Importing the same plugin twice registers primitives twice, so always bind once.

### Lists vs arrays

`map` and `filter` always return lists because Elle follows the Lisp tradition of structural recursion with `cons`/`first`/`rest`. Lists are the natural output of recursive accumulation.

**When to use which:**
- **Lists** — functional pipelines (`map`, `filter`, `fold`), recursive processing, variable-length results
- **Arrays** — indexed access, fixed-size data, interop with plugins expecting arrays

**Getting arrays from transforms:**
- `stream/into-array` — collect a stream into an array
- `mapcat` on arrays — preserves array type
- `map-indexed` on arrays — preserves array type
- Manual: accumulate into `@[]` with `push`, then `freeze`

### `protect` vs `try/catch`

They have different return conventions and error propagation:

```lisp
# try/catch — handle and recover
(try
  (risky-op)
  (catch e
    (fallback e)))       # returns fallback value

# protect — capture as data, never propagates
(def [ok? val] (protect (risky-op)))
(if ok? (use val) (handle val))

# defer — guaranteed cleanup, then propagates
(defer (cleanup)
  (risky-op))           # error continues unwinding after cleanup
```

| Aspect | `try/catch` | `protect` | `defer` |
|--------|------------|----------|--------|
| On success | Body value | `[true value]` | Body value |
| On error | Handler result | `[false error]` | Propagates |
| Error escapes? | Only if handler re-raises | Never | Always |
| Use case | Recovery | Safe capture | Resource cleanup |

All three are macros over `fiber/new` with mask `1` (`SIG_ERROR`). The difference is what happens after the fiber completes: `try/catch` runs a handler, `protect` wraps the result, `defer` calls `fiber/propagate` to continue unwinding.

### Streams are lazy coroutines

Streams are **lazy, demand-driven coroutines**. Each `coro/resume` pulls exactly one value. Transforms like `stream/map` and `stream/filter` return new coroutines that wrap a source — nothing executes until you pull.

```lisp
# Infinite stream — works because it's lazy
(defn naturals []
  (coro/new (fn []
    (var n 0)
    (forever
      (yield n)
      (assign n (+ n 1))))))

# Compose lazily, consume finitely
(stream/collect
  (stream/take 5
    (stream/map (fn [x] (* x x))
      (naturals))))
# => (0 1 4 9 16)
```

**Sink combinators** (`stream/collect`, `stream/fold`, `stream/for-each`, `stream/into-array`) consume a stream to completion — don't call them on infinite streams without `stream/take`.

**Port streams** (`port/lines`, `port/chunks`) yield `SIG_IO` under the hood. User code runs inside the async scheduler by default, so this works out of the box.

---

## Running and Testing

```bash
elle script.lisp          # run a file
echo '(+ 1 2)' | elle     # one-liner
elle                       # REPL

make smoke                 # ~15s: run examples
make test                  # ~2min: build + examples + unit tests
cargo test --workspace     # ~30min: full suite (ask first)
```
