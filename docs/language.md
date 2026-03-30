# Elle Language Guide

A practical guide to Elle's core language. For the complete reference, see
[QUICKSTART.md](../QUICKSTART.md).

## Table of Contents

1. [Critical Gotchas](#critical-gotchas)
2. [Basic Types](#basic-types)
3. [Collections](#collections)
4. [Bindings and Scope](#bindings-and-scope)
5. [Functions](#functions)
6. [Control Flow](#control-flow)
7. [Error Handling](#error-handling)
8. [Higher-Order Functions](#higher-order-functions)
9. [I/O and Subprocesses](#io-and-subprocesses)
10. [Concurrency](#concurrency)
11. [Modules and Imports](#modules-and-imports)
12. [Signals and Fibers](#signals-and-fibers)
13. [Traits](#traits)
14. [Common Patterns](#common-patterns)

---

## Critical Gotchas

Read these first. They are the most common sources of bugs.

### `nil` vs `()` — causes infinite loops

`nil` is falsy (absence of value). `()` is the empty list and is **truthy**.
Lists terminate with `()`, not `nil`.

```lisp
(nil? ())    # => false
(empty? ())  # => true
```

**Always use `empty?` to check end-of-list.** Using `nil?` as a loop
termination condition causes infinite recursion.

### `#` is comment, `;` is splice

```lisp
# This is a comment
(+ 1 ;[2 3] 4)  # => (+ 1 2 3 4)  — splice spreads into surrounding form
[1 ;[2 3] 4]    # => [1 2 3 4]    — works in collection literals too
```

### `assign` mutates; `set` creates a set

```lisp
(var x 10)
(assign x 20)   # correct: mutates x
(set x 20)      # WRONG: creates a set collection
```

### Only `nil` and `false` are falsy

```lisp
(if 0   :t :f)  # => :t  (0 is truthy)
(if ""  :t :f)  # => :t  (empty string is truthy)
(if ()  :t :f)  # => :t  (empty list is truthy)
(if nil :t :f)  # => :f
```

### Bare delimiters = immutable; `@`-prefix = mutable

```lisp
[1 2 3]    # array (immutable)
@[1 2 3]   # @array (mutable)
{:a 1}     # struct (immutable)
@{:a 1}    # @struct (mutable)
"hello"    # string (immutable)
@"hello"   # @string (mutable)
|1 2 3|    # set (immutable)
@|1 2 3|   # @set (mutable)
```

`put` on immutable returns a new copy. `put` on mutable mutates in place.

### `let` is parallel; `let*` is sequential

`let` evaluates all right-hand sides in the outer scope. Use `let*` when a
binding depends on a previous one.

```lisp
# WRONG: y cannot see x
(let [[x 5] [y (* x 2)]]
  y)

# RIGHT: let* makes x visible to y
(let* [[x 5] [y (* x 2)]]
  y)   # => 10
```

---

## Basic Types

```lisp
nil                  # absence of value (falsy)
true  false          # booleans (not #t/#f)
42                   # integer (64-bit signed)
3.14                 # float
0xFF  0o755  0b1010  # hex, octal, binary
1_000_000            # underscores allowed
:keyword             # self-evaluating keyword
'symbol              # quoted symbol
()                   # empty list (truthy)
```

### Type checking

```lisp
(type-of 42)         # => :integer
(type-of 3.14)       # => :float
(type-of "hi")       # => :string
(type-of :kw)        # => :keyword
(type-of nil)        # => :nil
(type-of [1 2])      # => :array
(type-of {:a 1})     # => :struct
(type-of (fn [] 1))  # => :closure

```

```text
# Predicates
(nil? x)      (boolean? x)   (number? x)
(integer? x)  (float? x)     (symbol? x)
(keyword? x)  (string? x)    (pair? x)
(list? x)     (empty? x)     (array? x)
(struct? x)   (bytes? x)     (set? x)
(fn? x)       (closure? x)   (native-fn? x)
(mutable? x)  (immutable? x)
(zero? x)     (pos? x)       (neg? x)
```

### Numbers

```lisp
(+ 1 2 3)   (- 10 3)   (* 2 3 4)
(/ 10 2)               # integer division truncates
(/ 7.0 2)              # => 3.5  (mixed: promotes to float)
(mod 10 3)             # => 1   (flooring)
(rem 10 3)             # => 1   (truncating)
(abs -5)
(min 3 1 4)  (max 3 1 4)
(even? 4)    (odd? 3)

(math/sqrt 16)   (math/pow 2 10)
(math/floor 3.7) (math/ceil 3.2)  (math/round 3.5)
(math/sin 1.0)   (math/cos 1.0)   (math/atan2 1.0 1.0)
(math/pi)        (math/e)
```

### Strings

Strings are immutable sequences of grapheme clusters. All string operations
use the `string/` prefix.

```lisp
(length "hello")                   # => 5 (grapheme count)
(string/size-of "hello")           # => 5 (byte count)
(get "hello" 0)                    # => "h"
(slice "hello" 1 4)                # => "ell"

# Concatenation
(string "hello " "world")          # => "hello world"
(string "count: " 42)              # => "count: 42"
(string/join ["a" "b" "c"] ",")    # => "a,b,c"

# Operations
(string/upcase "hello")            # => "HELLO"
(string/downcase "HELLO")          # => "hello"
(string/trim "  hi  ")             # => "hi"
(string/split "a,b,c" ",")         # => ["a" "b" "c"]
(string/contains? "hello" "ell")   # => true
(string/starts-with? "hello" "he") # => true
(string/ends-with? "hello" "lo")   # => true
(string/find "hello" "ll")         # => 2 (or nil)
(string/replace "foo-bar" "-" "_") # => "foo_bar"
(string/repeat "-" 40)             # => "----...----"
(string/format "{} + {} = {}" 1 2 3) # => "1 + 2 = 3"
```

### Conversions

```lisp
(integer "42")             (float "3.14")
(integer "ff" 16)          # => 255 (radix 2-36)
(string 42)                # => "42"
(string :foo)              # => "foo" (no colon)
(number->string 255 16)    # => "ff"
```

---

## Collections

### Lists (linked)

```lisp
(list 1 2 3)
(cons 1 (list 2 3))        # => (1 2 3)
(first (list 1 2 3))       # => 1
(rest (list 1 2 3))        # => (2 3)
(last (list 1 2 3))        # => 3
(length (list 1 2 3))      # => 3
(append (list 1 2) (list 3 4))  # => (1 2 3 4)
(reverse (list 1 2 3))     # => (3 2 1)
(take 2 (list 1 2 3))      # => (1 2)
(drop 1 (list 1 2 3))      # => (2 3)
(empty? (list 1 2 3))      # => false
(empty? ())                # => true  — use this, not nil?
```

### Arrays

```lisp
[1 2 3]                    # array (immutable)
@[1 2 3]                   # @array (mutable)

(length [1 2 3])           # => 3
(get [1 2 3] 1)            # => 2
(put [1 2 3] 0 99)         # => [99 2 3] (new array)

# Mutable arrays
(push @[1 2 3] 4)          # appends, mutates in place
(pop @[1 2 3])             # removes and returns last element
```

### Structs

Structs use keyword keys. Structs are **callable** with a keyword to get a
field.

```lisp
{:a 1 :b 2}               # struct (immutable)
@{:a 1 :b 2}              # @struct (mutable)

# Access
(get {:a 1} :a)            # => 1
(get {:a 1} :b :default)   # => :default
({:a 1 :b 2} :a)           # => 1  — callable struct syntax

# Accessor syntax — obj:field is sugar for (get obj :field)
(def config {:host "localhost" :port 8080})
config:host                # => "localhost"
config:port                # => 8080

# Update (immutable returns new struct)
(put {:a 1} :b 2)          # => {:a 1 :b 2}
(del {:a 1 :b 2} :a)       # => {:b 2}
(update {:count 5} :count inc) # => {:count 6}
(merge {:x 1 :y 2} {:y 3 :z 4})  # => {:x 1 :y 3 :z 4}

# Introspection
(keys {:a 1 :b 2})         # => (:a :b)
(values {:a 1 :b 2})       # => (1 2)
(has? {:a 1} :a)           # => true
(length {:a 1 :b 2})       # => 2

# Nested access and update
(get-in {:a {:b 1}} [:a :b])          # => 1
(put-in {:a {:b 1}} [:a :b] 2)        # => {:a {:b 2}}
(update-in {:a {:b 5}} [:a :b] inc)   # => {:a {:b 6}}

# Mutable @structs: put/del mutate in place
(put @{:a 1} :b 2)        # mutates in place
```

### Sets

```lisp
|1 2 3|                    # set (immutable)
@|1 2 3|                   # @set (mutable)

(contains? |1 2 3| 2)     # => true
(def s1 |1 2 3|)
(def s2 |2 3 4|)
(union s1 s2)              # => |1 2 3 4|
(intersection s1 s2)       # => |2 3|
(difference s1 s2)         # => |1|
```

### Bytes

```lisp
(bytes 1 2 3)              # immutable bytes
(@bytes 1 2 3)             # mutable bytes
(bytes "hello")            # string to bytes (UTF-8)
(get (bytes 1 2 3) 0)      # => 1
(length (bytes 1 2 3))     # => 3
(seq->hex (bytes 1 2 3))   # => "010203"
(string (bytes 97 98 99))  # => "abc"
```

### @Strings (mutable)

```lisp
@"hello"                   # mutable string
(thaw "hello")             # string -> @string
(freeze @"hello")          # @string -> string
(get @"hello" 0)           # => "h" (grapheme cluster)
(put @"hello" 0 "H")      # replaces grapheme at index
(push @"hello" "!")        # appends
(pop @"hello")             # removes and returns last grapheme
```

### Mutability conversion

```lisp
(freeze @[1 2 3])          # => [1 2 3]  (shallow)
(thaw [1 2 3])             # => @[1 2 3] (shallow)
(deep-freeze @[@[1] @{:a 2}])  # => [[1] {:a 2}] (recursive)
```

### Boxes (mutable cells)

```lisp
(def b (box 42))
(unbox b)           # => 42
(rebox b 99)
(unbox b)           # => 99
```

---

## Bindings and Scope

```lisp
# Top-level immutable (top-level is under implicit letrec)
(def x 42)

# Top-level mutable
(var x 42)
(assign x 100)

# Local (parallel bindings — RHS sees outer scope only)
(let [[x 10] [y 20]]
  (+ x y))

# Sequential (each binding sees previous)
(let* [[x 5] [y (* x 2)]]
  (+ x y))   # => 15

# Recursive
(letrec [[f (fn [] (g))]
         [g (fn [] 42)]]
  (f))
```

### Destructuring

Destructuring is strict — missing elements signal an error. Works in `def`,
`var`, `let`, `let*`, `fn`, `defn`, `match`.

```text
# List
(def (a b c) (list 1 2 3))
(def (hd & tl) (list 1 2 3))       # hd=1, tl=(2 3)

# Array (& rest collects into an array)
(def [x y] [10 20])
(def [fst & rst] [1 2 3])          # rst=[2 3]

# Struct
(def {:x x :y y} {:x 5 :y 10})

# Struct remainder
(def {:a a & more} {:a 1 :b 2 :c 3})  # a=1, more={:b 2 :c 3}

# Wildcard
(def (_ mid _) (list 10 20 30))    # mid=20

# Nested
(def {:point [_ y]} {:point [:skip :target]})  # y=:target

# In function parameters
(defn magnitude [{:x x :y y}]
  (+ x y))
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

# Variadic (& rest)
(defn sum-all [& nums]
  (fold + 0 nums))

# Closures capture lexical environment
(defn make-adder [n]
  (fn [x] (+ x n)))
(def add5 (make-adder 5))
(add5 10)   # => 15
```

### Optional positional parameters

```lisp
(defn greet [name &opt greeting]
  (println (or greeting "Hello") ", " name "!"))
(greet "Alice")         # Hello, Alice!
(greet "Bob" "Hey")     # Hey, Bob!
```

### Named keyword parameters

```lisp
(defn connect [host port &named timeout]
  [host port timeout])
(connect "localhost" 8080 :timeout 30)  # => ["localhost" 8080 30]
(connect "localhost" 8080)              # => ["localhost" 8080 nil]
```

Use `default` to set default values for named parameters:

```lisp
(defn open-window [&named title width height]
  (default title "Elle")
  (default width 800)
  (default height 600)
  {:title title :width width :height height})
```

### Keyword args collected as a struct

```lisp
(defn request [method path &keys opts]
  [method path opts])
(request "GET" "/" :timeout 30 :headers {:accept "text/html"})
# => ["GET" "/" {:timeout 30 :headers {:accept "text/html"}}]
```

---

## Control Flow

### Conditionals

```text
# If-then-else
(if test then else)

# Multi-branch
(cond
  ((> x 10) :large)
  ((> x 0)  :small)
  (true     :zero))

# One-armed
(when   test body...)
(unless test body...)

# Conditional binding (runs body only if expr is truthy)
(when-let [[val (get config :debug)]]
  (println "debug:" val))

(if-let [[conn (try-connect host)]]
  (use conn)
  (println "failed to connect"))

# Equality dispatch
(case x
  1 :one
  2 :two
  :other)
```

### Pattern matching

```text
# Compiler warns on non-exhaustive match;
# any unbound symbol works as a wildcard
(match value
  ([a b c] (+ a b c))
  ({:x x}  x)
  (_       :default))
```

### Loops

```lisp
# Iteration (each is the main loop form)
(each x in [1 2 3]
  (println x))
(each x [1 2 3]       # `in` is optional sugar
  (println x))

# While loop — (break) exits directly
(var i 0)
(while (< i 10)
  (assign i (+ i 1)))

# Repeat N times
(repeat 5 (println "hi"))
```

```text
# Infinite loop
(forever
  (process)
  (when done (break)))

# While-let — loop while binding succeeds
(while-let [[line (port/read-line port)]]
  (println line))
```

### Block and break

```lisp
# Block creates a new scope
(block
  (var x 10)
  x)
```

```text
# Named block with early exit
(block :search
  (each item in items
    (when (= item target)
      (break :search item)))
  nil)
```

### Sequencing

```text
# begin shares surrounding scope (no new scope created)
(begin expr1 expr2 ...)

# block creates a new scope
(block expr1 expr2 ...)
```

Use `begin` unless you need `break` or scope isolation.

---

## Error Handling

Elle does **not** have `try/catch/finally`. It uses `protect`, `defer`, and
`try/catch` (which is a macro over fibers).

```text
# Raise an error
(error {:error :bad-input :message "expected a number"})

# Catch with try/catch — e is a struct with :error and :message
(try
  (risky-op)
  (catch e
    (get e :error)    # => :division-by-zero etc.
    (get e :message)))

# Capture as data — returns [ok? value]
(def [ok? val] (protect (/ 10 0)))
(if ok?
  (use val)
  (handle-error val))

# Conditional error handling
(when-ok [result (parse-json input)]
  (println "parsed:" result))
# Returns nil if expr errors

# Guaranteed cleanup — defer runs cleanup on scope exit
(defer (close f)
  (use f))

# Resource management — with is defer + constructor binding
(with f (open "data.txt") close
  (read-all f))
```

| Form | On success | On error | Error escapes? | Use case |
|------|-----------|---------|---------------|----------|
| `try/catch` | Body value | Handler result | Only if handler re-raises | Recovery |
| `protect` | `[true value]` | `[false error]` | Never | Safe capture |
| `defer` | Body value | Propagates | Always | Resource cleanup |

---

## Higher-Order Functions

```text
(map    f [1 2 3])           # => (2 4 6)  — always returns list
(filter f [1 2 3 4])         # => (3 4)    — always returns list
(fold   f init [1 2 3])      # => result
(apply  f [1 2 3])           # => (f 1 2 3)
```

```lisp
(sum [1 2 3 4])              # => 10
(product [1 2 3 4])          # => 24

# Type conversion
(->array (list 1 2 3))      # => [1 2 3]
(->list [1 2 3])             # => (1 2 3)

# Sorting
(sort [3 1 2])               # => (1 2 3)  — natural order
(sort-by length ["bb" "a" "ccc"])  # => ("a" "bb" "ccc")
(sort-with (fn [a b] (compare b a)) [3 1 2])  # => (3 2 1)

# Threading macros
(-> 5 (+ 10) (* 2))         # => 30  (insert as first arg)
(->> [1 2 3] (map (fn [x] (* x 2))))  # (insert as last arg)
```

Note: `map` and `filter` always return lists, even when given arrays. Use
`->array` to convert back, or `stream/into-array` for streams.

---

## I/O and Subprocesses

### File I/O

```text
# Read a file
(def p (port/open "data.txt" :read))
(defer (port/close p)
  (println (port/read-all p)))

# Read line by line
(def p (port/open "data.txt" :read))
(defer (port/close p)
  (each line in (stream/collect (port/lines p))
    (println line)))

# Write a file
(def p (port/open "output.txt" :write))
(defer (port/close p)
  (port/write p "hello world\n"))

# Port operations
(port/read p n)              # read n bytes
(port/write p data)          # write bytes or string
(port/read-line p)           # read until \n, nil on EOF
(port/read-all p)            # read everything
(port/seek p offset)         # seek to byte offset
(port/tell p)                # current byte position
(port/close p)               # close port
(port/flush p)               # flush buffers
```

### Subprocesses

```text
# Run to completion — returns {:exit :stdout :stderr}
(subprocess/system "echo" ["hello"])
# => {:exit 0 :stdout "hello\n" :stderr ""}

# With options
(subprocess/system "ls" ["-la"] {:cwd "/tmp"})
(subprocess/system "env" [] {:env {:FOO "bar"}})

# Low-level control
(def proc (subprocess/exec "cat" []))
(subprocess/pid proc)
(subprocess/wait proc)
(subprocess/kill proc)
```

### Output

```text
(print "no newline")           # write to *stdout*
(println "with newline")       # write to *stdout* + newline
(println "count: " 42)         # multiple args concatenated
(eprint "to stderr")           # write to *stderr*
(eprintln "error: bad input")  # write to *stderr* + newline
(pp value)                     # pretty-print data structures
```

All output functions are async (they yield to the scheduler).

---

## Concurrency

User code runs inside the async scheduler automatically. Use `ev/spawn` and
`ev/join` for concurrency, not threads.

```text
# Spawn and join
(def f (ev/spawn (fn [] (port/read-all (port/open "data.txt" :read)))))
(def content (ev/join f))

# Join a sequence — results in input order
(def [a b] (ev/join [(ev/spawn (fn [] (fetch "/users")))
                      (ev/spawn (fn [] (fetch "/posts")))]))

# Parallel map
(ev/map (fn [url] (http/get url)) urls)

# Protected join — returns [ok? value] instead of raising
(let [[[ok? val] (ev/join-protected (ev/spawn (fn [] (flaky-call))))]]
  (if ok? val (fallback)))

# Race — first to complete wins, abort the rest
(ev/race [(ev/spawn (fn [] (query-replica-1)))
          (ev/spawn (fn [] (query-replica-2)))])

# Timeout
(ev/timeout 5 (fn [] (http/get "https://slow.example.com")))

# Scoped concurrency — children cannot outlive scope
(ev/scope (fn [spawn]
  (let [[users    (spawn (fn [] (fetch "/users")))]
        [settings (spawn (fn [] (fetch "/settings")))]]
    {:users (ev/join users) :settings (ev/join settings)})))

# Sleep
(ev/sleep 1.5)   # yield for 1.5 seconds
```

### TCP

```text
(tcp/listen addr port)      # bind and listen
(tcp/accept listener)       # yield until connection
(tcp/connect host port)     # yield until connected
```

### Channels

```text
(def [tx rx] (chan))        # unbounded channel
(def [tx rx] (chan 10))     # bounded (capacity 10)

(chan/send tx 42)           # => [:ok], [:full], or [:disconnected]
(chan/recv rx)              # => [:ok msg], [:empty], or [:disconnected]
(chan/close tx)             # close sender
```

### Dynamic parameters

```lisp
(def *my-param* (make-parameter :default-value))
(*my-param*)                # => :default-value

(parameterize ((*my-param* :overridden))
  (*my-param*))             # => :overridden
```

---

## Modules and Imports

```text
# Import by short name — searches ELLE_PATH, ELLE_HOME, and CWD
(def http ((import "lib/http")))       # finds lib/http.lisp
(def crypto (import "crypto"))         # finds libelle_crypto.so

# Full path
(import "lib/http.lisp")
(import-file "lib/http.lisp")         # alias

# Plugin — returns a struct of functions
(def crypto (import "crypto"))
(seq->hex (crypto:sha256 "hello"))

# Destructure plugin
(def {:sha256 sha256 :hmac-sha256 hmac} (import "crypto"))
(seq->hex (sha256 "hello"))
```

Every `import` call reloads the file fresh. Bind once at top level to
avoid redundant loads.

---

## Signals and Fibers

Signals are the unified mechanism for non-local control flow. Every signal
is a bit in a mask.

**Built-in signals:** `:error` (bit 0), `:yield` (bit 1), `:debug` (bit 2),
`:ffi` (bit 4), `:halt` (bit 8), `:io` (bit 9), `:exec` (bit 11),
`:fuel` (bit 12), `:wait` (bit 14).

```lisp
# Create a fiber with a signal mask (set literals are preferred)
(def f (fiber/new (fn [] (yield 42)) |:yield|))

# Resume, delivering a value
(fiber/resume f nil)

# Inspect
(fiber/status f)   # :new :alive :suspended :dead :error
(fiber/value f)    # signal value
(fiber/bits f)     # signal bits
(fiber/mask f)     # signal mask
```

```text
# Terminate
(fiber/cancel f)   # hard kill (no unwinding)
(fiber/abort f)    # graceful (with unwinding)
```

Signal masks accept set literals `|:yield :io|`, keywords `:yield`, arrays
`[:yield :io]`, or raw integers `2`. Set literals are preferred.

### `silence` vs `squelch`

- **`silence`** is a compile-time declaration in a function preamble.
  `(silence f)` constrains parameter `f` to be completely silent.
- **`squelch`** is a runtime primitive. `(squelch f :yield)` returns a new
  closure that catches `:yield` at runtime.

```lisp
# silence — compile-time contract
(defn safe-map [f xs]
  (silence f)
  (map f xs))
```

```text
# squelch — runtime wrapper
(let [[safe-f (squelch f |:yield|)]]
  (map safe-f xs))
```

For more details, see [signals](signals.md).

---

## Traits

Traits attach metadata to values. Any heap-allocated value can carry a trait
table (an immutable struct).

```lisp
# Attach traits
(def v (with-traits [1 2 3] {:type :point :dim 2}))

# Read traits
(traits v)   # => {:type :point :dim 2}
(traits [1 2 3])  # => nil (no traits)
```

Traits are invisible to structural equality, ordering, and hashing.

---

## Common Patterns

### Library closure pattern

Elle libraries wrap code in a closure returning a struct of functions. This
allows parameterization and avoids polluting global scope.

```text
## ── lib/counter.lisp ──────────────────────────────────────────────
(fn []
  (var n 0)
  (defn inc-count [] (assign n (+ n 1)) n)
  (defn get-count [] n)
  {:inc inc-count :count get-count})
```

Usage:

```text
(def counter ((import-file "lib/counter.lisp")))
(counter:inc)    # => 1
(counter:inc)    # => 2
(counter:count)  # => 2
```

### Tail-recursive processing

Tail call optimization is guaranteed.

```lisp
(defn sum-list [lst acc]
  (if (empty? lst)
    acc
    (sum-list (rest lst) (+ acc (first lst)))))
(sum-list [1 2 3 4 5] 0)   # => 15
```

### Mutable accumulator

```lisp
(defn make-counter []
  (var n 0)
  (fn []
    (assign n (+ n 1))
    n))
```

### Safe error capture

```text
(def [ok? val] (protect (risky-op)))
(if ok?
  (use val)
  (handle-error val))
```

### Struct update

```text
(update state :count inc)
(merge state {:count (+ (get state :count) 1)})
(update-in config [:db :pool-size] inc)
```

### Iterate with index

```text
(var i 0)
(each x in items
  (println i x)
  (assign i (+ i 1)))
```

### Immediate-mode GUI loop

```text
(var count 0)
(def win (ui:open :title "Counter"))
(ui:run win (fn [ix]
  (when (ui:clicked? ix :inc) (assign count (inc count)))
  (ui:v-layout
    (ui:heading (string "Count: " count))
    (ui:button :inc "+"))))
```

---

## Macros

```text
# Quasiquote-based
(defmacro when [test & body]
  `(if ,test (begin ,;body) nil))

# syntax-case for structural matching
(defmacro swap! [a b]
  (syntax-case (list a b)
    ([x y]
     `(let [[tmp ,x]]
        (assign ,x ,y)
        (assign ,y tmp)))))
```

---

## What Doesn't Exist

| Missing | Use instead |
|---------|-------------|
| `string-append` | `(string "a" x "b")` |
| `string-upcase` | `(string/upcase s)` |
| `string-split` | `(string/split s delim)` |
| `struct-get` | `(get s :key)` or `(s :key)` or `s:key` |
| `@struct "key" val` | `@{:key val}` |
| `lambda` (idiomatic) | `fn` |
| `for` loop | `each` |
| `-e` flag | `echo '...' \| elle` |
| `set!` / `set` for mutation | `assign` |
| `#t` / `#f` | `true` / `false` |
| `define` | `def` / `defn` / `var` |
| `display` | `print` |
| `null` | `nil` |
| `char` type | strings are grapheme-indexed |
| `function?` | `fn?` |
| threads for concurrency | `ev/spawn` / `ev/join` |
