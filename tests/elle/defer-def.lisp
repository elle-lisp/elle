(elle/epoch 12)
# defer-def: a nested defer's body must run, not fault.
#
# THIS TEST FAILS. It exits 1 with
#   {:error :type-error :message "fiber/resume: expected fiber, got nil"}
# naming an internal invariant the author cannot act on. It goes green when
# the defect is fixed. `ht.md` carries the analysis notes.
#
# Four ingredients, all required together — drop any one and the script
# passes. Each is pinned by its own assertion below, so this file also states
# which lever to pull:
#
#   1. an enclosing `defer`;
#   2. a `def` (not `let`) inside that defer's body;
#   3. whose initializer is a CALL, not a constant;
#   4. a nested `defer` whose body BOTH reads that `def` AND calls a
#      top-level `defn`.
#
# `with-temp-dir` matches this shape because it expands through `with`,
# which is a `defer` — the temp directory itself is incidental, so nothing
# here touches the filesystem. The working rule for authors: inside a
# `defer` scope, bind with `let`, never `def`.

(defn increment [x]
  (+ x 1))

# ── The discriminators: each drops exactly one ingredient and passes ──

# No enclosing defer.
(let [root "/x"]
  (def bare-path (path/join root "a"))
  (defer
    (assert true "cleanup runs")
    (assert (string? bare-path) "without an enclosing defer, the def reads back")
    (assert (= 2 (increment 1))
            "without an enclosing defer, the defn is callable")))

# A def bound to a constant rather than a call.
(let [root "/x"]
  (defer
    (assert true "outer cleanup runs")
    (def constant-path "/x/a")
    (defer
      (assert true "cleanup runs")
      (assert (string? constant-path) "a constant def reads back")
      (assert (= 2 (increment 1)) "a constant def leaves the defn callable"))))

# A let in place of the def.
(let [root "/x"]
  (defer
    (assert true "outer cleanup runs")
    (let [let-path (path/join root "a")]
      (defer
        (assert true "cleanup runs")
        (assert (string? let-path) "a let binding reads back")
        (assert (= 2 (increment 1)) "a let binding leaves the defn callable")))))

# A nested body that reads the def but calls no top-level defn.
(let [root "/x"]
  (defer
    (assert true "outer cleanup runs")
    (def read-only-path (path/join root "a"))
    (defer
      (assert true "cleanup runs")
      (assert (string? read-only-path) "reading the def alone is fine"))))

# A nested body that calls the defn but never reads the def.
(let [root "/x"]
  (defer
    (assert true "outer cleanup runs")
    (def unread-path (path/join root "a"))
    (defer
      (assert true "cleanup runs")
      (assert (= 2 (increment 1)) "calling the defn alone is fine"))))

# ── The pin: all four ingredients at once ──

(let [root "/x"]
  (defer
    (assert true "outer cleanup runs")
    (def joined-path (path/join root "a"))
    (defer
      (assert true "cleanup runs")
      (assert (string? joined-path)
              "the def is readable from the nested defer body")
      (assert (= 2 (increment 1))
              "the top-level defn is callable from the nested defer body"))))
