# Error Handling

<!-- audited: 2026-09-23 -->

Errors in Elle are values signaled via fibers. By convention, error values
are structs `{:error :keyword :message "string"}`, but `(error val)` accepts
any value — integers, strings, lists. Catch handlers that assume struct
shape should guard with `struct?` first.

```lisp
(assert (= (protect (error 42)) [false 42]))
```

## Error struct convention

Error values are structs with three standard fields:

```lisp
(def http-failure
  {:error   :http-error              # module/category — which subsystem failed
   :reason  :malformed-header        # specific condition — what went wrong
   :message "malformed header"})     # human-readable summary (for logs/REPL)
```

**`:error`** is the genus. Match on this for broad catch-all handling:
"is this an HTTP problem, a DNS problem, or something else?"

**`:reason`** is the species. Match on this for targeted recovery:
"was it a malformed header, an unsupported scheme, or an EOF?"

**`:message`** is prose for humans. It must never contain information
that isn't already in a struct field. Programs should never need to
parse the message string — every datum is in its own field.

Additional fields carry context values relevant to the specific error:

```text
# Good: every datum is a field; message is a formatted summary
{:error :dns-format-error
 :reason :bad-rdata-length
 :rtype :a
 :expected 4
 :actual 7
 :message "A record rdata length is not 4"}

# Bad: information only in the message string
{:error :dns-error
 :message "dns: A record rdata length is not 4"}
```

### Matching on errors

A broad handler matches `:error`; a targeted one matches `:reason` and reads
the context fields:

```lisp
(defn fetch [url]
  (error {:error :http-error :reason :malformed-header :url url
          :message "malformed header"}))

(defn join-channel [nick]
  (error {:error :irc-error :reason :nick-collision :nick nick
          :message "nick taken"}))

(def broad
  (try (fetch "http://example.test/")
    (catch e
      (when (= e:error :http-error)
        (string "HTTP failed: " e:message)))))
(assert (= broad "HTTP failed: malformed header"))

(def targeted
  (try (join-channel "ada")
    (catch e
      (when (= e:reason :nick-collision)
        (string "nick " e:nick " taken, trying another")))))
(assert (= targeted "nick ada taken, trying another"))
```

## Raising errors

`(error val)` signals an error carrying `val`:

```lisp
(defn check-age [age]
  (when (< age 0)
    (error {:error :bad-input :reason :negative-age :value age
            :message "expected a non-negative age"}))
  age)

(def [age-ok? age-err] (protect (check-age -1)))
(assert (not age-ok?))
(assert (= age-err:reason :negative-age))
```

## try / catch

`try` runs the body; if an error occurs, the catch handler runs with the
error bound to the catch variable.

```lisp
(def result (try
  (/ 1 0)
  (catch e
    (string "caught: " e:message))))
(assert (= result "caught: /: division by zero"))
```

When no error occurs, `try` returns the body's value:

```lisp
(assert (= (try (+ 10 20) (catch e :nope)) 30))
```

## protect — errors as data

`protect` captures errors without propagating. Returns `[ok? value]`.

```lisp
(def [ok? val] (protect (+ 100 200)))
(assert ok?)
(assert (= val 300))

(def [ok2? err] (protect (/ 1 0)))
(assert (not ok2?))
(assert (= err:error :division-by-zero))
```

A common pattern — try something, fall back on failure:

```lisp
(defn safe-parse [s]
  (def [parsed? n] (protect (parse-int s)))
  (if parsed? n nil))

(assert (= (safe-parse "42") 42))
(assert (nil? (safe-parse "abc")))
```

## when-ok

Bind + branch in one step: runs body only if expr succeeds, returns `nil`
if it errors.

```lisp
(assert (= (when-ok [x (+ 1 2)]
             (* x 10))
           30))

(assert (nil? (when-ok [x (error "oops")]
                (* x 10))))
```

## defer — guaranteed cleanup

`(defer cleanup body…)` runs `cleanup` after the body, whether the body
succeeds or errors, and returns the body's value.

```lisp
(def log @[])
(def deferred
  (defer (push log :cleanup)
    (push log :body)
    42))
(assert (= deferred 42))
(assert (= log @[:body :cleanup]))
```

On error, cleanup runs, then the error re-propagates:

```lisp
(def err-log @[])
(def caught
  (try
    (defer (push err-log :cleanup)
      (push err-log :body)
      (error {:error :fail :reason :oops :message "oops"}))
    (catch e :caught)))
(assert (= caught :caught))
(assert (= err-log @[:body :cleanup]))
```

## with — resource management

`with` acquires a resource, runs body, then releases via a destructor.
The destructor runs even on error.

```lisp
(def rlog @[])

(defn open-conn []
  (push rlog :opened)
  {:type :conn :id 1})

(defn close-conn [c]
  (push rlog :closed))

(def conn-id
  (with conn (open-conn) close-conn
    (push rlog :used)
    conn:id))
(assert (= conn-id 1))
(assert (= rlog @[:opened :used :closed]))
```

## Cleanup around async I/O

The four forms hold their order when the body waits on the scheduler.
`protect` and `try` capture an error from an async call — a timed-out
`tcp/accept`, a `port/open` on a fifo nobody reads — and the code after
the capture runs before any enclosing cleanup:

```lisp
(def listener (tcp/listen "127.0.0.1" 0))
(def order @[])

(with-temp-dir dir
  (def [ok? err] (protect (tcp/accept listener :timeout 20)))
  (assert (not ok?) "nobody connects, so the accept fails")
  (file/write (path/join dir "note") "the directory is still here")
  (push order :body))

(assert (= (freeze order) [:body]) "the body ran to its end")
(port/close listener)
```

`with-temp-dir` deletes the directory when the body ends, not when the
body's own error handling ends. The same holds for `with`, for a `defer`
inside a `defer`, and for a body that does more I/O after the capture.
[tests/elle/unwind-suspend.lisp](../tests/elle/unwind-suspend.lisp) pins the
order for each shape.

## Error propagation

Errors bubble up through the call stack until caught.

```lisp
(defn validate [age]
  (when (< age 0)
    (error {:error :invalid :reason :negative-age :value age
            :message "negative age"}))
  age)

(defn make-person [name age]
  {:name name :age (validate age)})

# the error propagates from validate through make-person
(def person-err (try (make-person "Bob" -5) (catch e e)))
(assert (= person-err:error :invalid))
(assert (= person-err:reason :negative-age))
(assert (= person-err:value -5))
```

## protect vs try/catch vs defer

| Form | On success | On error | Use case |
|------|------------|----------|----------|
| try/catch | Body value | Handler result | Recovery |
| protect | `[true value]` | `[false error]` | Safe capture |
| defer | Body value | Propagates | Resource cleanup |

---

## See also

- [signals](signals/index.md) — signal system underlying errors
- [fibers](signals/fibers.md) — fiber error states and masks
- [control.md](control.md) — conditionals and loops
