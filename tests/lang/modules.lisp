(elle/epoch 15)
# audited: 2026-10-26
# import loads a module file as a function of its parameters; each import runs the file again, and a call answers its exports.
# docs/modules.md

# ============================================================================
# 1. Basic parametric import with qualified symbol access
# ============================================================================

(let [fmt ((import "../modules/formatter") :prefix "[" :suffix "]"
      :separator " | ")]
  (assert (= (fmt:wrap "hello") "[hello]")
          "qualified access: wrap with prefix/suffix")
  (assert (= (fmt:join [1 2 3]) "1 | 2 | 3")
          "qualified access: join with separator")
  (assert (= (fmt:upper "hello") "HELLO")
          "qualified access: upper (unconfigured)")
  (assert (= (fmt:identity 42) 42) "qualified access: identity"))

# ============================================================================
# 2. Two instances with different configurations
# ============================================================================

(let [brackets ((import "../modules/formatter") :prefix "(" :suffix ")")
      angles ((import "../modules/formatter") :prefix "<" :suffix ">")]
  (assert (= (brackets:wrap "x") "(x)") "two instances: brackets wrap")
  (assert (= (angles:wrap "x") "<x>") "two instances: angles wrap")
  (assert (= (brackets:join ["a" "b"]) "a, b")
          "two instances: brackets default separator")
  (assert (= (angles:join ["a" "b"]) "a, b")
          "two instances: angles default separator"))

# ============================================================================
# 3. Default parameters (no keyword args)
# ============================================================================

(let [fmt ((import "../modules/formatter"))]
  (assert (= (fmt:wrap "hello") "hello")
          "defaults: wrap with empty prefix/suffix")
  (assert (= (fmt:join ["a" "b" "c"]) "a, b, c")
          "defaults: join with default separator"))

# ============================================================================
# 4. Selective destructuring import
# ============================================================================

(let [{:wrap wrap :upper upper} ((import "../modules/formatter") :prefix "<<"
      :suffix ">>")]
  (assert (= (wrap "hi") "<<hi>>") "destructured: wrap")
  (assert (= (upper "hi") "HI") "destructured: upper"))

# ============================================================================
# 5. Module as first-class value
# ============================================================================

(defn apply-wrap [mod s]
  "Call wrap from a module struct."
  (mod:wrap s))

(let [fmt ((import "../modules/formatter") :prefix "{" :suffix "}")]
  (assert (= (apply-wrap fmt "val") "{val}")
          "first-class: pass module to function"))

# ============================================================================
# 6. Isolation — a definition in a module does not leak into the importer
# ============================================================================

# A module is one form, and what it defines is local to that form. The only
# way to get a definition out is the value. The stdlib has an `inc` of one
# argument, so a call of none fails unless the module's `inc` of none leaked
# into a later compile.
((import "../modules/counter"))
(let [[ok? err] (protect (eval '(inc)))]
  (assert (not ok?) "defn in imported file is not visible in caller scope")
  (assert (string/contains? (get err :message) "inc expects 1 argument")
          "the call reached the stdlib inc, not the module's"))

# ============================================================================
# 7. Existing module fixtures
# ============================================================================

# test.lisp — simple value exports
(let [test-mod ((import "../modules/test"))]
  (assert (= test-mod:test-var 42) "test.lisp: test-var")
  (assert (= test-mod:test-string "hello") "test.lisp: test-string")
  (assert (= test-mod:test-list (list 1 2 3)) "test.lisp: test-list"))

# a path that starts with ./ names the same file
(let [dotted ((import "./../modules/test"))]
  (assert (= dotted:test-var 42) "a ./ path imports the same module"))

# counter.lisp — each call of its function builds a counter of its own
(let [c1 ((import "../modules/counter"))
      c2 ((import "../modules/counter"))]
  (c1:inc)
  (c1:inc)
  (c1:inc)
  (assert (= (c1:count) 3) "counter: c1 incremented three times")
  (assert (= (c2:count) 0) "counter: c2 independent, still zero")
  (c2:inc)
  (assert (= (c2:count) 1) "counter: c2 incremented once"))

# ============================================================================
# 8. A `protect` in a module's form runs while the form loads
# ============================================================================

# The fixture's header holds the trap.
(let [m ((import "../modules/protect-toplevel"))]
  (assert (= (m:ok) true)
          "top-level protect module imports without leaking SIG_SWITCH"))
