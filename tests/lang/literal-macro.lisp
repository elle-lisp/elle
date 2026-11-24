(elle/epoch 19)
# audited: 2026-11-24
# A macro whose template holds constants expands the same way every time it is called.
# docs/macros.md
#
# The counter-factual: the compiler builds a template's constants once, so
# every expansion of a macro reads the same values. An expander that changed
# one in place would hand each later expansion what an earlier one did to
# it: a symbol carrying another expansion's scopes, or a list another
# expansion extended. A later call would then bind or read the wrong name.

# ── A string and a quoted symbol in a template ──────────────────────

(defmacro greet (name)
  `(string "hello, " ,name))
(defmacro tagged (x)
  `(pair 'tag ,x))

(defn greetings []
  [(greet "a") (greet "b")])

(assert (= (greetings) ["hello, a" "hello, b"])
        "each expansion reads the string")
(assert (= (greetings) ["hello, a" "hello, b"]) "and does on the next call")
(assert (= (greet "c") "hello, c") "an expansion outside the function too")
(assert (= [(tagged 1) (tagged 2)] [(pair 'tag 1) (pair 'tag 2)])
        "each expansion reads the quoted symbol")

(defn looped [n]
  "n expansions' worth of work, from one expansion inside a loop."
  (def acc @[])
  (var i 0)
  (while (< i n)
    (push acc (greet "x"))
    (assign i (+ i 1)))
  (freeze acc))

(assert (= (looped 3) ["hello, x" "hello, x" "hello, x"])
        "an expansion in a loop reads the string on every pass")

# ── Hygiene across expansions ───────────────────────────────────────

(defmacro my-swap (a b)
  `(let [tmp ,a]
     (assign ,a ,b)
     (assign ,b tmp)))

(def @tmp 100)
(def @x 1)
(def @y 2)
(my-swap x y)
(my-swap x y)
(my-swap x y)
(assert (= [x y tmp] [2 1 100]) "every expansion's tmp is the macro's own")

# Each level binds tmp around the body it passes to the next level, so a
# reference a level wrote must keep naming that level's binding. The count
# arrives as a syntax object, so each level reads it with syntax/e.
(defmacro nest (n body)
  (let [k (syntax/e n)]
    (if (= k 0)
      body
      `(let [tmp ,k]
         (nest ,(- k 1) (+ tmp ,body))))))

(assert (= (nest 3 0) 6) "each level's tmp names its own binding")
(defn nested []
  (nest 4 0))
(assert (= (nested) 10) "and does in a later expansion of the same template")
(assert (= (nest 3 0) 6) "and in the one after that")

(println "literal-macro: ok")
