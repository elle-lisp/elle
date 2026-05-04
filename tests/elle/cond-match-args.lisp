(elle/epoch 10)
# Regression tests for Bug 612: cond/match corrupt previously-evaluated
# arguments in variadic native function calls.

# ── cond as argument ──────────────────────────────────────────────────────

(assert (= (path/join "a"
                      (cond
                        true "b")) "a/b") "cond as second arg to path/join")

(assert (= (list 1
                 (cond
                   true 2) 3) (list 1 2 3)) "cond as second arg in list")

(assert (= (list 1 2
                 (cond
                   true 3)) (list 1 2 3)) "cond as third arg in list")

(assert (= (path/join "a"
                      (cond
                        false "x"
                        true "b")) "a/b") "cond multi-clause as second arg")

# ── match as argument ─────────────────────────────────────────────────────

(assert (= (path/join "a"
                      (match 1
                        1 "b"
                        _ "c")) "a/b") "match as second arg to path/join")

(assert (= (list 1
                 (match 1
                   1 2
                   _ 0) 3) (list 1 2 3)) "match as second arg in list")

(assert (= (list 1 2
                 (match 5
                   1 99
                   _ 3)) (list 1 2 3)) "match wildcard as third arg in list")

# ── if/cond as first arg ─────────────────────────────────────────────────

(assert (= (path/join (cond
                        true "a") "b") "a/b") "cond as first arg still works")

(assert (= (path/join "a" (if true "b" "c")) "a/b")
        "if as second arg still works")
