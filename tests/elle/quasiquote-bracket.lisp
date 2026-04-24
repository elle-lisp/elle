(elle/epoch 9)
# Quasiquote bracket support + when-ok macro
#
# Regression test: brackets inside quasiquote templates were quoted
# as-is (unquote not processed), so macro output couldn't use [[...]]
# binding syntax. Now arrays in quasiquote process unquote/splice and
# produce runtime arrays that round-trip to SyntaxKind::Array.

# ── 1. Bracket let in macro output ──────────────────────────────────

(defmacro bind1 (name val & body)
  `(let [,name ,val] ,;body))

(bind1 x 42
  (assert (= x 42) "1: bracket let"))
(println "1: bracket let ok")

# ── 2. Bracket let* in macro output ─────────────────────────────────

(defmacro bind2 (n1 v1 n2 v2 & body)
  `(let* [,n1 ,v1 ,n2 ,v2] ,;body))

(bind2 a 1 b 2
  (assert (= (+ a b) 3) "2: bracket let*"))
(println "2: bracket let* ok")

# ── 3. Array data in quasiquote still works ─────────────────────────

(defmacro make-pair (a b)
  `[,a ,b])

(def p (make-pair 10 20))
(assert (= (get p 0) 10) "3a: array data")
(assert (= (get p 1) 20) "3b: array data")
(println "3: array data ok")

# ── 4. protect still works (uses [...] as data in quasiquote) ───────

(def [ok? val] (protect (+ 1 2)))
(assert ok? "4a: protect ok?")
(assert (= val 3) "4b: protect val")
(println "4: protect ok")

# ── 5. when-ok success ──────────────────────────────────────────────

(def r (when-ok [v (+ 10 20)] (+ v 5)))
(assert (= r 35) "5: when-ok success")
(println "5: when-ok success ok")

# ── 6. when-ok failure ──────────────────────────────────────────────

(def @ran false)
(when-ok [v (error {:error :test :message "boom"})]
  (assign ran true))
(assert (not ran) "6: when-ok skip on error")
(println "6: when-ok error skip ok")

(println "all quasiquote-bracket tests passed")
