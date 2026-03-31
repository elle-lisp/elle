# Error Handling

Errors in Elle are values signaled via fibers. By convention, error values
are structs `{:error :keyword :message "string"}`, but any value works.

## Raising errors

```lisp
# (error val) signals an error
# (error {:error :bad-input :message "expected a number"})
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
# (when-ok [result (parse-json input)]
#   (println "parsed:" result))
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
    (error {:error :fail :message "oops"}))
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
    (error {:error :invalid :message "negative age"}))
  age)

(defn make-person [name age]
  {:name name :age (validate age)})

# error propagates from validate through make-person
(def err (try (make-person "Bob" -5) (catch e e)))
err:error                  # => :invalid
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

- [signals.md](signals.md) — signal system underlying errors
- [fibers.md](fibers.md) — fiber error states and masks
- [control.md](control.md) — conditionals and loops
