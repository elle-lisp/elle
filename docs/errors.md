# Error Handling

Errors in Elle are values signaled via fibers. By convention, error values
are structs `{:error :keyword :message "string"}`, but `(error val)` accepts
any value — integers, strings, lists. Catch handlers that assume struct
shape should guard with `struct?` first.

## Error struct convention

Error values are structs with three standard fields:

```lisp
{:error   :http-error              # module/category — which subsystem failed
 :reason  :malformed-header        # specific condition — what went wrong
 :message "malformed header"}      # human-readable summary (for logs/REPL)
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

```text
# Broad: catch all HTTP errors
(try (http:get url)
  (catch e
    (when (= e:error :http-error)
      (println "HTTP failed:" e:message))))

# Targeted: handle a specific condition
(try (irc:connect host port :nick nick)
  (catch e
    (when (= e:reason :nick-collision)
      (println "nick" e:nick "taken, trying another"))))
```

## Raising errors

```lisp
# (error val) signals an error
# (error {:error :bad-input :reason :negative-age :value age
#         :message "expected a non-negative age"})
```

## try / catch

`try` runs the body; if an error occurs, the catch handler runs with the
error bound to the catch variable.

```lisp
(def result (try
  (/ 1 0)
  (catch e
    (string "caught: " e:message))))
result                     # => "caught: division by zero"
```

When no error occurs, `try` returns the body's value:

```lisp
(try (+ 10 20) (catch e :nope))  # => 30
```

## protect — errors as data

`protect` captures errors without propagating. Returns `[ok? value]`.

```lisp
(def [ok? val] (protect (+ 100 200)))
ok?                        # => true
val                        # => 300

(def [ok2? err] (protect (/ 1 0)))
ok2?                       # => false
err:error                  # => :division-by-zero
```

A common pattern — try something, fall back on failure:

```lisp
(defn safe-parse [s]
  (def [ok? val] (protect (integer s)))
  (if ok? val nil))

(safe-parse "42")          # => 42
(safe-parse "abc")         # => nil
```

## when-ok

Bind + branch in one step: runs body only if expr succeeds, returns `nil`
if it errors.

```lisp
(when-ok [x (+ 1 2)]
  (* x 10))               # => 30

(when-ok [x (error "oops")]
  (* x 10))               # => nil
```

## defer — guaranteed cleanup

`defer` runs cleanup after body, whether body succeeds or errors.

```lisp
(def log @[])
(defer (push log :cleanup)
  (push log :body)
  42)
# log is now @[:body :cleanup]
# return value is 42
```

On error, cleanup runs, then the error re-propagates:

```lisp
(def err-log @[])
(try
  (defer (push err-log :cleanup)
    (push err-log :body)
    (error {:error :fail :reason :oops :message "oops"}))
  (catch e :caught))
# err-log is @[:body :cleanup]
# try returns :caught
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

(with conn (open-conn) close-conn
  (push rlog :used)
  conn:id)                 # => 1
# rlog is @[:opened :used :closed]
```

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

# error propagates from validate through make-person
(def err (try (make-person "Bob" -5) (catch e e)))
err:error                  # => :invalid
err:reason                 # => :negative-age
err:value                  # => -5
```

## protect vs try/catch vs defer

```text
              On success         On error            Use case
───────────────────────────────────────────────────────────────
try/catch     Body value         Handler result      Recovery
protect       [true value]       [false error]       Safe capture
defer         Body value         Propagates          Resource cleanup
```

---

## See also

- [signals](signals/index.md) — signal system underlying errors
- [fibers](signals/fibers.md) — fiber error states and masks
- [control.md](control.md) — conditionals and loops
